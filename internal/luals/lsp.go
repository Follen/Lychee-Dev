package luals

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

type QueryKind string

const (
	Definition QueryKind = "definition"
	References QueryKind = "references"
	Hover      QueryKind = "hover"
)

type Query struct {
	Kind     QueryKind `json:"kind"`
	Path     string    `json:"path"`
	Position Position  `json:"position"` // zero-based UTF-16
}
type Location struct {
	Path  string `json:"path"`
	Range Range  `json:"range"`
}
type QueryResult struct {
	Query     Query      `json:"query"`
	State     string     `json:"state"` // complete, unsupported, or incomplete
	Locations []Location `json:"locations,omitempty"`
	Hover     string     `json:"hover,omitempty"`
	Truncated bool       `json:"truncated,omitempty"`
	Reason    string     `json:"reason,omitempty"`
}
type Capabilities struct {
	Definition bool `json:"definition"`
	References bool `json:"references"`
	Hover      bool `json:"hover"`
}
type Analysis struct {
	Runtime          Identity      `json:"runtime"`
	State            string        `json:"state"` // complete or partial
	PositionEncoding string        `json:"positionEncoding"`
	Capabilities     Capabilities  `json:"capabilities"`
	Results          []QueryResult `json:"results"`
	Coverage         Coverage      `json:"coverage"`
	Config           []byte        `json:"-"`
}

type Coverage struct {
	LuaFiles      int      `json:"luaFiles"`
	LuaBytes      int64    `json:"luaBytes"`
	ExcludedFiles int      `json:"excludedFiles"`
	Reasons       []string `json:"reasons,omitempty"`
	probePath     string
}

// Analyze makes a bounded frozen copy for local unsaved addon closures.
func (r *Runtime) Analyze(ctx context.Context, files map[string][]byte, definitions []byte, queries []Query) (Analysis, error) {
	result := Analysis{Runtime: r.Identity, State: "partial", PositionEncoding: "utf-16", Results: []QueryResult{}}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	_, workspace, _, _, cleanup, err := frozenWorkspace(files, definitions)
	if err != nil {
		return result, err
	}
	defer cleanup()
	return r.AnalyzeWorkspace(ctx, workspace, definitions, queries)
}

// AnalyzeWorkspace reads an already verified fixed worktree in place. The
// caller holds its lease and verifies its source identity; no source file is
// written here. Query locations are mapped back to workspace-relative paths.
func (r *Runtime) AnalyzeWorkspace(ctx context.Context, workspaceRoot string, definitions []byte, queries []Query) (Analysis, error) {
	result := Analysis{Runtime: r.Identity, State: "partial", PositionEncoding: "utf-16", Results: []QueryResult{}}
	if len(queries) > 128 || len(definitions) > MaxDefinitionsBytes {
		return result, ErrBudget
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	workspace, err := filepath.Abs(workspaceRoot)
	if err != nil {
		return result, err
	}
	info, err := os.Stat(workspace)
	if err != nil || !info.IsDir() {
		return result, fmt.Errorf("%w: workspace unavailable", ErrUnavailable)
	}
	result.Coverage, err = workspaceCoverage(workspace)
	if err != nil {
		return result, err
	}
	for _, q := range queries {
		if !filepath.IsLocal(q.Path) || !strings.EqualFold(filepath.Ext(q.Path), ".lua") || q.Position.Line < 0 || q.Position.Character < 0 || q.Position.Line > 1000000 || q.Position.Character > 1000000 {
			return result, fmt.Errorf("%w: query path or position", ErrBudget)
		}
		if q.Kind != Definition && q.Kind != References && q.Kind != Hover {
			return result, fmt.Errorf("%w: query kind", ErrUnsupported)
		}
	}
	if len(queries) == 0 {
		result.State = "complete"
		return result, nil
	}
	dir, err := os.MkdirTemp("", "lycheedev-luals-lsp-")
	if err != nil {
		return result, err
	}
	defer os.RemoveAll(dir)
	library := filepath.Join(dir, "definitions")
	for _, p := range []string{library, filepath.Join(dir, "log"), filepath.Join(dir, "meta")} {
		if err = os.MkdirAll(p, 0700); err != nil {
			return result, err
		}
	}
	if err = os.WriteFile(filepath.Join(library, "api.d.lua"), definitions, 0600); err != nil {
		return result, err
	}
	result.Config, err = configuration(library)
	if err != nil {
		return result, err
	}
	config := filepath.Join(dir, "config.json")
	if err = os.WriteFile(config, result.Config, 0600); err != nil {
		return result, err
	}
	bounded, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	command := exec.CommandContext(bounded, filepath.Join(r.directory, "bin", "lua-language-server.exe"),
		"--configpath="+config, "--logpath="+filepath.Join(dir, "log"), "--metapath="+filepath.Join(dir, "meta"), "--quiet")
	command.Dir = dir
	command.WaitDelay = 2 * time.Second
	hideProcess(command)
	stdin, err := command.StdinPipe()
	if err != nil {
		return result, err
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		return result, err
	}
	var stderr cappedWriter
	command.Stderr = &stderr
	closeJob, err := startGuarded(command)
	if err != nil {
		return result, fmt.Errorf("%w: %v", ErrFailed, err)
	}
	defer func() { closeJob(); _ = stdin.Close(); _ = command.Wait() }()
	go func() { <-bounded.Done(); closeJob(); _ = stdin.Close() }()
	client := newLSPClient(bounded, stdin, stdout)
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
	if err = client.request("initialize", initParams, &init); err != nil {
		return result, clientError(err, stderr.String(), bounded, ctx)
	}
	if init.Capabilities.PositionEncoding != "" && init.Capabilities.PositionEncoding != "utf-16" {
		return result, fmt.Errorf("%w: unsupported position encoding %q", ErrUnsupported, init.Capabilities.PositionEncoding)
	}
	result.Capabilities = Capabilities{
		Definition: providerEnabled(init.Capabilities.DefinitionProvider),
		References: providerEnabled(init.Capabilities.ReferencesProvider),
		Hover:      providerEnabled(init.Capabilities.HoverProvider),
	}
	if err = client.notify("initialized", map[string]any{}); err != nil {
		return result, clientError(err, stderr.String(), bounded, ctx)
	}
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
		if err = client.request("textDocument/definition", map[string]any{"textDocument": map[string]string{"uri": probeURI}, "position": Position{}}, &barrier); err != nil {
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
		if err = client.request(method, params, &raw); err != nil {
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
	_ = client.request("shutdown", nil, nil)
	_ = client.notify("exit", nil)
	return result, nil
}

func clientError(err error, stderr string, bounded, parent context.Context) error {
	if parent.Err() != nil {
		return parent.Err()
	}
	if bounded.Err() != nil {
		return fmt.Errorf("%w: 90 second deadline", ErrFailed)
	}
	return fmt.Errorf("%w: %v (%s)", ErrFailed, err, stderr)
}

func providerEnabled(raw json.RawMessage) bool {
	return len(raw) > 0 && string(raw) != "false" && string(raw) != "null"
}

func fileURI(path string) string {
	p := filepath.ToSlash(filepath.Clean(path))
	if len(p) > 1 && p[1] == ':' {
		p = "/" + p
	}
	return (&url.URL{Scheme: "file", Path: p}).String()
}

func pathFromURI(uri, workspace, library string) (string, error) {
	u, err := url.Parse(uri)
	if err != nil || u.Scheme != "file" || u.Host != "" {
		return "", errors.New("invalid file URI")
	}
	p := u.Path
	if len(p) > 2 && p[0] == '/' && p[2] == ':' {
		p = p[1:]
	}
	p = filepath.Clean(filepath.FromSlash(p))
	for _, root := range []string{workspace, library} {
		rel, err := filepath.Rel(root, p)
		if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			if root == library {
				return "@environment/" + filepath.ToSlash(rel), nil
			}
			return filepath.ToSlash(rel), nil
		}
	}
	return "", errors.New("location outside analysis workspace")
}

func decodeLocations(raw json.RawMessage, workspace, library string) ([]Location, bool, error) {
	result := []Location{}
	if len(raw) == 0 || string(raw) == "null" {
		return result, false, nil
	}
	var items []json.RawMessage
	if raw[0] == '[' {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, false, err
		}
	} else {
		items = []json.RawMessage{raw}
	}
	truncated := len(items) > 1000
	if truncated {
		items = items[:1000]
	}
	for _, item := range items {
		var loc struct {
			URI         string `json:"uri"`
			Range       Range  `json:"range"`
			TargetURI   string `json:"targetUri"`
			TargetRange Range  `json:"targetRange"`
		}
		if err := json.Unmarshal(item, &loc); err != nil {
			return nil, false, err
		}
		uri, span := loc.URI, loc.Range
		if uri == "" {
			uri, span = loc.TargetURI, loc.TargetRange
		}
		path, err := pathFromURI(uri, workspace, library)
		if err != nil {
			return nil, false, err
		}
		if span.Start.Line < 0 || span.Start.Character < 0 || span.End.Line < span.Start.Line || span.End.Character < 0 || (span.End.Line == span.Start.Line && span.End.Character < span.Start.Character) {
			return nil, false, errors.New("invalid LSP location range")
		}
		result = append(result, Location{Path: path, Range: span})
	}
	return result, truncated, nil
}

func decodeHover(raw json.RawMessage) (string, bool, error) {
	if len(raw) == 0 || string(raw) == "null" {
		return "", false, nil
	}
	var value struct {
		Contents json.RawMessage `json:"contents"`
	}
	if err := json.Unmarshal(raw, &value); err != nil {
		return "", false, err
	}
	if len(value.Contents) == 0 {
		return "", false, errors.New("hover lacks contents")
	}
	plain, err := hoverContents(value.Contents)
	if err != nil {
		return "", false, err
	}
	if len(plain) <= 16384 {
		return plain, false, nil
	}
	cut := 16384
	for cut > 0 && !utf8.RuneStart(plain[cut]) {
		cut--
	}
	return plain[:cut], true, nil
}

func hoverContents(raw json.RawMessage) (string, error) {
	var plain string
	if json.Unmarshal(raw, &plain) == nil {
		return plain, nil
	}
	var marked struct {
		Value string `json:"value"`
	}
	if json.Unmarshal(raw, &marked) == nil && marked.Value != "" {
		return marked.Value, nil
	}
	var list []json.RawMessage
	if json.Unmarshal(raw, &list) == nil {
		parts := []string{}
		for _, item := range list {
			part, err := hoverContents(item)
			if err != nil {
				return "", err
			}
			if part != "" {
				parts = append(parts, part)
			}
		}
		return strings.Join(parts, "\n"), nil
	}
	return "", errors.New("unsupported hover contents")
}

type rpcMessage struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method,omitempty"`
	Params  json.RawMessage `json:"params,omitempty"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *struct {
		Code    int    `json:"code"`
		Message string `json:"message"`
	} `json:"error,omitempty"`
}
type rpcEvent struct {
	message rpcMessage
	err     error
}
type lspClient struct {
	ctx    context.Context
	input  io.Writer
	events chan rpcEvent
	nextID int
	bytes  int
}

func newLSPClient(ctx context.Context, input io.Writer, output io.Reader) *lspClient {
	c := &lspClient{ctx: ctx, input: input, events: make(chan rpcEvent, 1)}
	go func() {
		reader := bufio.NewReader(output)
		for {
			body, err := readFrame(reader)
			if err != nil {
				select {
				case c.events <- rpcEvent{err: err}:
				case <-ctx.Done():
				}
				return
			}
			var msg rpcMessage
			if err = json.Unmarshal(body, &msg); err != nil {
				select {
				case c.events <- rpcEvent{err: err}:
				case <-ctx.Done():
				}
				return
			}
			select {
			case c.events <- rpcEvent{message: msg}:
			case <-ctx.Done():
				return
			}
		}
	}()
	return c
}

func readFrame(reader *bufio.Reader) ([]byte, error) {
	length := -1
	headerBytes := 0
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			return nil, err
		}
		headerBytes += len(line)
		if headerBytes > 8192 {
			return nil, ErrBudget
		}
		line = strings.TrimSpace(line)
		if line == "" {
			break
		}
		if strings.HasPrefix(strings.ToLower(line), "content-length:") {
			length, err = strconv.Atoi(strings.TrimSpace(line[len("content-length:"):]))
			if err != nil {
				return nil, err
			}
		}
	}
	if length < 0 || length > MaxReportBytes {
		return nil, ErrBudget
	}
	body := make([]byte, length)
	_, err := io.ReadFull(reader, body)
	return body, err
}

func (c *lspClient) send(value any) error {
	body, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(body) > MaxReportBytes {
		return ErrBudget
	}
	_, err = c.input.Write(append([]byte(fmt.Sprintf("Content-Length: %d\r\n\r\n", len(body))), body...))
	return err
}
func (c *lspClient) notify(method string, params any) error {
	return c.send(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
}
func (c *lspClient) request(method string, params any, result any) error {
	c.nextID++
	id := c.nextID
	if err := c.send(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params}); err != nil {
		return err
	}
	for {
		select {
		case <-c.ctx.Done():
			return c.ctx.Err()
		case event := <-c.events:
			if event.err != nil {
				return event.err
			}
			c.bytes += len(event.message.Params) + len(event.message.Result)
			if c.bytes > 32<<20 {
				return ErrBudget
			}
			msg := event.message
			if msg.Method != "" {
				if len(msg.ID) > 0 {
					var response any
					switch msg.Method {
					case "workspace/configuration":
						response = []any{}
					case "window/workDoneProgress/create", "client/registerCapability", "client/unregisterCapability":
						response = nil
					default:
						return fmt.Errorf("unexpected server request %q", msg.Method)
					}
					if err := c.send(map[string]any{"jsonrpc": "2.0", "id": json.RawMessage(msg.ID), "result": response}); err != nil {
						return err
					}
				}
				continue
			}
			var got int
			if err := json.Unmarshal(msg.ID, &got); err != nil || got != id {
				return fmt.Errorf("unexpected response id %s", msg.ID)
			}
			if msg.Error != nil {
				return fmt.Errorf("server error %d: %s", msg.Error.Code, msg.Error.Message)
			}
			if result != nil {
				if err := json.Unmarshal(msg.Result, result); err != nil {
					return err
				}
			}
			return nil
		}
	}
}

func workspaceCoverage(root string) (Coverage, error) {
	coverage := Coverage{Reasons: []string{"XML inline Lua is outside LuaLS workspace coverage; provide explicit virtual Lua files when needed"}}
	nodes := 0
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		nodes++
		if nodes > 100000 {
			return fmt.Errorf("%w: workspace directory budget", ErrBudget)
		}
		if entry.IsDir() {
			if path != root && (entry.Name() == ".git" || entry.Name() == "node_modules") {
				return filepath.SkipDir
			}
			return nil
		}
		if entry.Type()&os.ModeSymlink != 0 {
			coverage.ExcludedFiles++
			return nil
		}
		if !strings.EqualFold(filepath.Ext(path), ".lua") {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			coverage.ExcludedFiles++
			return nil
		}
		coverage.LuaFiles++
		coverage.LuaBytes += info.Size()
		if info.Size() > MaxFileBytes {
			coverage.ExcludedFiles++
		} else if coverage.probePath == "" {
			coverage.probePath, err = filepath.Rel(root, path)
			if err != nil {
				return err
			}
		}
		if coverage.LuaFiles > MaxWorkspaceFiles || coverage.LuaBytes > MaxWorkspaceBytes {
			return fmt.Errorf("%w: workspace exceeds LuaLS preload budget", ErrBudget)
		}
		return nil
	})
	if err != nil {
		return coverage, err
	}
	if coverage.ExcludedFiles > 0 {
		coverage.Reasons = append(coverage.Reasons, "Lua files larger than 1 MiB or links were excluded from LuaLS preload")
	}
	return coverage, nil
}

func readQueryFile(workspace, name string) ([]byte, error) {
	path := filepath.Join(workspace, name)
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > MaxFileBytes {
		return nil, ErrBudget
	}
	root, err := os.OpenRoot(workspace)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	file, err := root.Open(name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, MaxFileBytes+1))
	if err != nil || len(data) > MaxFileBytes {
		return nil, ErrBudget
	}
	return data, nil
}
