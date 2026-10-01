package luals

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// WorkspaceSession owns one guarded server and its bounded private artifacts.
// Composition owns and retains the verified workspace lease until Close returns.
// A session has one semantic context; callers serialize batches. Any protocol
// error, cancellation or deadline retires it before another batch is admitted.
type WorkspaceSession struct {
	runtime            *Runtime
	workspace, library string
	definitions        []byte
	result             Analysis
	client             *lspClient
	stdin              io.WriteCloser
	command            *exec.Cmd
	closeJob           func()
	cancel             context.CancelFunc
	cleanup            func()
	artifacts          string
	watches            []func()
	once               sync.Once
	batches            int
	batchMu            sync.Mutex
	mu                 sync.Mutex
	closed             bool
}

func (r *Runtime) OpenWorkspaceSession(ctx context.Context, workspaceRoot string, definitions []byte) (*WorkspaceSession, error) {
	return r.openWorkspaceSession(ctx, workspaceRoot, definitions, false, "")
}
func (r *Runtime) OpenBrokerWorkspaceSession(ctx context.Context, workspaceRoot string, definitions []byte, artifactParent string) (*WorkspaceSession, error) {
	return r.openWorkspaceSession(ctx, workspaceRoot, definitions, true, artifactParent)
}
func (r *Runtime) openWorkspaceSession(ctx context.Context, workspaceRoot string, definitions []byte, broker bool, artifactParent string) (session *WorkspaceSession, err error) {
	startup, stopStartup := context.WithTimeout(ctx, 90*time.Second)
	defer stopStartup()
	ctx = startup
	result := Analysis{Runtime: r.Identity, State: "partial", PositionEncoding: "utf-16", Results: []QueryResult{}}
	if len(definitions) > MaxDefinitionsBytes {
		return nil, ErrBudget
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	var cleanup func()
	workspace, err := filepath.Abs(workspaceRoot)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(workspace)
	if err != nil || !info.IsDir() {
		return nil, fmt.Errorf("%w: workspace unavailable", ErrUnavailable)
	}
	result.Coverage, err = workspaceCoverage(workspace)
	if err != nil {
		return nil, err
	}
	dir, err := os.MkdirTemp(artifactParent, "lycheedev-luals-lsp-")
	if err != nil {
		return nil, err
	}
	cleanup = func() { _ = os.RemoveAll(dir) }
	defer func() {
		if err != nil {
			cleanup()
		}
	}()
	library := filepath.Join(dir, "definitions")
	for _, p := range []string{library, filepath.Join(dir, "log"), filepath.Join(dir, "meta")} {
		if err = os.MkdirAll(p, 0700); err != nil {
			return nil, err
		}
	}
	if err = os.WriteFile(filepath.Join(library, "api.d.lua"), definitions, 0600); err != nil {
		return nil, err
	}
	result.Config, err = configuration(library)
	if err != nil {
		return nil, err
	}
	config := filepath.Join(dir, "config.json")
	if err = os.WriteFile(config, result.Config, 0600); err != nil {
		return nil, err
	}

	life, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	command := exec.CommandContext(life, filepath.Join(r.directory, "bin", "lua-language-server.exe"), "--configpath="+config, "--logpath="+filepath.Join(dir, "log"), "--metapath="+filepath.Join(dir, "meta"), "--quiet")
	command.Dir = dir
	command.WaitDelay = 2 * time.Second
	hideProcess(command)
	stdin, err := command.StdinPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		cancel()
		return nil, err
	}
	var stderr cappedWriter
	command.Stderr = &stderr
	closeJob, err := startGuardedPolicy(command, broker)
	if err != nil {
		cancel()
		return nil, fmt.Errorf("%w: %v", ErrFailed, err)
	}
	session = &WorkspaceSession{runtime: r, workspace: workspace, library: library, definitions: append([]byte(nil), definitions...), result: result, stdin: stdin, command: command, closeJob: closeJob, cancel: cancel, cleanup: cleanup, artifacts: dir}
	owned := session
	stop := context.AfterFunc(ctx, func() { owned.Close() })
	defer stop()
	context.AfterFunc(life, func() { owned.Close() })
	client := newLSPClient(life, stdin, stdout)
	session.client = client
	go func() { <-client.done; owned.Close() }()
	defer func() {
		if err != nil {
			owned.Close()
		}
	}()
	if broker {
		err = session.installWatch(dir, watchSessionArtifacts)
		if err != nil {
			return nil, err
		}
	}
	if broker {
		err = session.installWatch(workspace, watchWorkspace)
		if err != nil {
			return nil, err
		}
	}
	rootURI := fileURI(workspace)
	var init struct {
		Capabilities struct {
			DefinitionProvider json.RawMessage `json:"definitionProvider"`
			ReferencesProvider json.RawMessage `json:"referencesProvider"`
			HoverProvider      json.RawMessage `json:"hoverProvider"`
			PositionEncoding   string          `json:"positionEncoding"`
		} `json:"capabilities"`
	}
	initParams := map[string]any{
		"processId": os.Getpid(), "rootUri": rootURI, "rootPath": workspace,
		"workspaceFolders": []map[string]string{{"uri": rootURI, "name": filepath.Base(workspace)}},
		"capabilities": map[string]any{
			"general":      map[string]any{"positionEncodings": []string{"utf-16"}},
			"workspace":    map[string]any{"configuration": false, "workspaceFolders": true},
			"textDocument": map[string]any{"definition": map[string]any{"linkSupport": false}, "references": map[string]any{}, "hover": map[string]any{"contentFormat": []string{"markdown", "plaintext"}}},
		},
	}
	if err = client.requestContext(ctx, "initialize", initParams, &init); err != nil {
		return nil, clientError(err, stderr.String(), ctx, ctx)
	}
	if init.Capabilities.PositionEncoding != "" && init.Capabilities.PositionEncoding != "utf-16" {
		return nil, fmt.Errorf("%w: unsupported position encoding %q", ErrUnsupported, init.Capabilities.PositionEncoding)
	}
	result.Capabilities = Capabilities{
		Definition: providerEnabled(init.Capabilities.DefinitionProvider),
		References: providerEnabled(init.Capabilities.ReferencesProvider),
		Hover:      providerEnabled(init.Capabilities.HoverProvider),
	}
	if err = client.notify("initialized", map[string]any{}); err != nil {
		return nil, clientError(err, stderr.String(), ctx, ctx)
	}

	session.result = result
	return session, nil
}

func (s *WorkspaceSession) Close() error {
	s.once.Do(func() {
		s.mu.Lock()
		s.closed = true
		watches := s.watches
		s.watches = nil
		s.mu.Unlock()
		s.cancel()
		s.closeJob()
		_ = s.stdin.Close()
		_ = s.command.Wait()
		for _, stop := range watches {
			stop()
		}
		s.cleanup()
	})
	return nil
}

func (s *WorkspaceSession) Analyze(ctx context.Context, queries []Query) (result Analysis, err error) {
	s.batchMu.Lock()
	defer s.batchMu.Unlock()
	result = s.result
	result.Results = []QueryResult{}
	result.State = "partial"
	s.mu.Lock()
	closed := s.closed
	s.mu.Unlock()
	if closed {
		return result, fmt.Errorf("%w: session retired", ErrUnavailable)
	}
	if len(queries) > 128 {
		return result, ErrBudget
	}
	for _, q := range queries {
		if !filepath.IsLocal(q.Path) || !strings.EqualFold(filepath.Ext(q.Path), ".lua") || q.Position.Line < 0 || q.Position.Character < 0 || q.Position.Line > 1000000 || q.Position.Character > 1000000 {
			return result, fmt.Errorf("%w: query path or position", ErrBudget)
		}
		if q.Kind != Definition && q.Kind != References && q.Kind != Hover {
			return result, ErrUnsupported
		}
	}
	if s.batches >= 256 {
		s.Close()
		return result, ErrBudget
	}
	s.batches++
	if err = s.checkSealedArtifacts(); err != nil {
		s.Close()
		return result, err
	}
	if err = checkSessionArtifacts(s.artifacts); err != nil {
		s.Close()
		return result, err
	}
	if err = s.client.beginBatch(); err != nil {
		s.Close()
		return result, err
	}
	bounded, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	stop := context.AfterFunc(bounded, func() { s.Close() })
	defer stop()
	defer func() {
		if err != nil {
			s.Close()
		}
	}()
	workspace, library, definitions, client := s.workspace, s.library, s.definitions, s.client
	var stderr cappedWriter
	opened := map[string]bool{}
	// LuaLS waits for the scope of each queried URI, not every workspace.
	// The generated API declaration lives in an external library and can map
	// to the fallback scope while the source worktree is still preloading.
	// A request against a worktree URI is an event-driven preload barrier:
	// LuaLS's definition provider awaits that scope before replying. The
	// worktree preload also loads the configured generated API library.
	if result.Coverage.probePath != "" {
		probeData, readErr := readQueryFile(workspace, result.Coverage.probePath)
		if readErr != nil {
			return result, fmt.Errorf("%w: workspace preload probe unavailable: %v", ErrFailed, readErr)
		}
		probeURI := fileURI(filepath.Join(workspace, result.Coverage.probePath))
		if err = client.notify("textDocument/didOpen", map[string]any{"textDocument": map[string]any{"uri": probeURI, "languageId": "lua", "version": 1, "text": string(probeData)}}); err != nil {
			return result, clientError(err, stderr.String(), bounded, ctx)
		}
		opened[probeURI] = true
		var barrier json.RawMessage
		if err = client.requestContext(bounded, "textDocument/definition", map[string]any{"textDocument": map[string]string{"uri": probeURI}, "position": Position{}}, &barrier); err != nil {
			return result, clientError(err, stderr.String(), bounded, ctx)
		}
	}
	for _, q := range queries {
		entry := QueryResult{Query: q, State: "incomplete", Locations: []Location{}}
		supported := q.Kind == Definition && result.Capabilities.Definition || q.Kind == References && result.Capabilities.References || q.Kind == Hover && result.Capabilities.Hover
		if !supported {
			entry.State = "unsupported"
			entry.Reason = "server did not advertise capability"
			result.Results = append(result.Results, entry)
			continue
		}
		path := filepath.Join(workspace, q.Path)
		var data []byte
		var readErr error
		if q.Path == DefinitionPath {
			path = filepath.Join(library, "api.d.lua")
			data = definitions
		} else {
			data, readErr = readQueryFile(workspace, q.Path)
		}
		if readErr != nil {
			entry.Reason = "query file unavailable or exceeds file budget"
			result.Results = append(result.Results, entry)
			continue
		}
		uri := fileURI(path)
		if !opened[uri] {
			if err = client.notify("textDocument/didOpen", map[string]any{"textDocument": map[string]any{"uri": uri, "languageId": "lua", "version": 1, "text": string(data)}}); err != nil {
				return result, clientError(err, stderr.String(), bounded, ctx)
			}
			opened[uri] = true
		}
		params := map[string]any{"textDocument": map[string]string{"uri": uri}, "position": q.Position}
		method := "textDocument/" + string(q.Kind)
		if q.Kind == References {
			params["context"] = map[string]bool{"includeDeclaration": true}
		}
		var raw json.RawMessage
		if err = client.requestContext(bounded, method, params, &raw); err != nil {
			return result, clientError(err, stderr.String(), bounded, ctx)
		}
		if q.Kind == Hover {
			entry.Hover, entry.Truncated, err = decodeHover(raw)
		} else {
			entry.Locations, entry.Truncated, err = decodeLocations(raw, workspace, library)
		}
		if err != nil {
			return result, fmt.Errorf("%w: %v", ErrFailed, err)
		}
		entry.State = "complete"
		if entry.Truncated {
			entry.State = "incomplete"
			entry.Reason = "semantic result exceeded output budget"
		}
		result.Results = append(result.Results, entry)
	}
	result.State = "complete"
	if result.Coverage.ExcludedFiles > 0 {
		result.State = "partial"
	}
	for _, entry := range result.Results {
		if entry.State != "complete" {
			result.State = "partial"
			break
		}
	}

	for uri := range opened {
		if err = client.notify("textDocument/didClose", map[string]any{"textDocument": map[string]string{"uri": uri}}); err != nil {
			return result, err
		}
	}
	if err = checkSessionArtifacts(s.artifacts); err != nil {
		return result, err
	}
	return result, nil
}
func (s *WorkspaceSession) PID() int { return s.command.Process.Pid }

func (s *WorkspaceSession) Usable() bool { s.mu.Lock(); defer s.mu.Unlock(); return !s.closed }

// SplitQueries preserves the existing per-direction budgets when a composed
// relation request contains references/hover plus 128 outgoing call sites.
// Each RPC batch remains at most 128 queries; all share one initialized worker.
func SplitQueries(queries []Query) [][]Query {
	batches := [][]Query{}
	for len(queries) > 0 {
		n := min(128, len(queries))
		batches = append(batches, queries[:n])
		queries = queries[n:]
	}
	return batches
}
func (r *Runtime) AnalyzeWorkspaceBatches(ctx context.Context, workspace string, definitions []byte, batches [][]Query) (Analysis, error) {
	bounded, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	ctx = bounded
	result := Analysis{Runtime: r.Identity, State: "partial", Results: []QueryResult{}}
	if len(batches) > 2 {
		return result, ErrBudget
	}
	for _, batch := range batches {
		if len(batch) > 128 {
			return result, ErrBudget
		}
	}
	session, err := r.OpenWorkspaceSession(ctx, workspace, definitions)
	if err != nil {
		return result, err
	}
	defer session.Close()
	return session.AnalyzeBatches(ctx, batches)
}
func (s *WorkspaceSession) AnalyzeBatches(ctx context.Context, batches [][]Query) (Analysis, error) {
	result := Analysis{Runtime: s.runtime.Identity, State: "complete", Results: []QueryResult{}}
	if len(batches) > 2 {
		return result, ErrBudget
	}
	for _, batch := range batches {
		if len(batch) > 128 {
			return result, ErrBudget
		}
	}
	for _, batch := range batches {
		part, err := s.Analyze(ctx, batch)
		result.Results = append(result.Results, part.Results...)
		result.Coverage = part.Coverage
		result.PositionEncoding = part.PositionEncoding
		result.Capabilities = part.Capabilities
		result.Config = part.Config
		if err != nil {
			result.State = "partial"
			return result, err
		}
		if part.State != "complete" {
			result.State = "partial"
		}
	}
	return result, nil
}

func (s *WorkspaceSession) installWatch(dir string, watch func(string, func()) (func(), error)) error {
	stop, err := watch(dir, func() { s.Close() })
	if err != nil {
		return err
	}
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		stop()
		return ErrUnavailable
	}
	s.watches = append(s.watches, stop)
	s.mu.Unlock()
	return nil
}

func (s *WorkspaceSession) BatchCount() int {
	s.batchMu.Lock()
	defer s.batchMu.Unlock()
	return s.batches
}

func (s *WorkspaceSession) checkSealedArtifacts() error {
	for _, file := range []struct {
		path     string
		expected []byte
	}{{filepath.Join(s.library, "api.d.lua"), s.definitions}, {filepath.Join(s.artifacts, "config.json"), s.result.Config}} {
		info, err := os.Lstat(file.path)
		if err != nil || !info.Mode().IsRegular() || info.Size() != int64(len(file.expected)) {
			return ErrUnavailable
		}
		opened, err := os.Open(file.path)
		if err != nil {
			return err
		}
		raw, readErr := io.ReadAll(io.LimitReader(opened, int64(len(file.expected))+1))
		opened.Close()
		if readErr != nil {
			return readErr
		}
		if !bytes.Equal(raw, file.expected) {
			return ErrUnavailable
		}
	}
	return nil
}
