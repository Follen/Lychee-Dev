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
	"path/filepath"
	"strconv"
	"strings"
	"sync"
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
	bounded, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	ctx = bounded
	result := Analysis{Runtime: r.Identity, State: "partial", PositionEncoding: "utf-16", Results: []QueryResult{}}
	if len(queries) > 128 || len(definitions) > MaxDefinitionsBytes {
		return result, ErrBudget
	}
	if len(queries) == 0 {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		coverage, err := workspaceCoverage(workspaceRoot)
		result.Coverage = coverage
		if err == nil {
			result.State = "complete"
		}
		return result, err
	}
	session, err := r.OpenWorkspaceSession(ctx, workspaceRoot, definitions)
	if err != nil {
		return result, err
	}
	defer session.Close()
	return session.Analyze(ctx, queries)
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
	ctx           context.Context
	input         io.Writer
	writeMu       sync.Mutex
	mu            sync.Mutex
	pending       map[int]chan rpcEvent
	nextID        int
	batchBytes    int
	lifetimeBytes int
	terminal      error
	done          chan struct{}
}

// The pump keeps consuming notifications and answering supported server
// requests even between batches. Payloads are discarded, never queued for an
// idle caller. Every inbound and outbound byte has a session-wide bound.
func newLSPClient(ctx context.Context, input io.Writer, output io.Reader) *lspClient {
	c := &lspClient{ctx: ctx, input: input, pending: make(map[int]chan rpcEvent), done: make(chan struct{})}
	go func() {
		defer close(c.done)
		reader := bufio.NewReader(output)
		for {
			body, err := readFrame(reader)
			if err != nil {
				c.fail(err)
				return
			}
			if err = c.account(len(body)); err != nil {
				c.fail(err)
				return
			}
			var msg rpcMessage
			if err = json.Unmarshal(body, &msg); err != nil || msg.JSONRPC != "2.0" {
				c.fail(fmt.Errorf("invalid RPC message: %v", err))
				return
			}
			if msg.Method != "" {
				if len(msg.ID) > 0 {
					var response any
					switch msg.Method {
					case "workspace/configuration":
						response = []any{}
					case "window/workDoneProgress/create", "client/registerCapability", "client/unregisterCapability":
						response = nil
					default:
						c.fail(fmt.Errorf("unexpected server request %q", msg.Method))
						return
					}
					if err = c.send(map[string]any{"jsonrpc": "2.0", "id": msg.ID, "result": response}); err != nil {
						c.fail(err)
						return
					}
				}
				continue
			}
			var id int
			if err = json.Unmarshal(msg.ID, &id); err != nil {
				c.fail(fmt.Errorf("invalid response id %s", msg.ID))
				return
			}
			c.mu.Lock()
			ch := c.pending[id]
			delete(c.pending, id)
			c.mu.Unlock()
			if ch == nil {
				c.fail(fmt.Errorf("unexpected response id %s", msg.ID))
				return
			}
			ch <- rpcEvent{message: msg}
		}
	}()
	return c
}
func (c *lspClient) account(n int) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.batchBytes += n
	c.lifetimeBytes += n
	if c.batchBytes > 32<<20 || c.lifetimeBytes > 256<<20 {
		return ErrBudget
	}
	return nil
}
func (c *lspClient) beginBatch() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.terminal != nil {
		return c.terminal
	}
	c.batchBytes = 0
	return nil
}
func (c *lspClient) fail(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.terminal != nil {
		return
	}
	c.terminal = err
	for id, ch := range c.pending {
		ch <- rpcEvent{err: err}
		delete(c.pending, id)
	}
}
func readFrame(reader *bufio.Reader) ([]byte, error) {
	length := -1
	headerBytes := 0
	for {
		lineBytes, err := reader.ReadSlice('\n')
		line := string(lineBytes)
		if errors.Is(err, bufio.ErrBufferFull) {
			return nil, ErrBudget
		}
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
	if err = c.account(len(body)); err != nil {
		return err
	}
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	_, err = c.input.Write(append([]byte(fmt.Sprintf("Content-Length: %d\r\n\r\n", len(body))), body...))
	return err
}
func (c *lspClient) notify(method string, params any) error {
	return c.send(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
}
func (c *lspClient) request(method string, params any, result any) error {
	return c.requestContext(c.ctx, method, params, result)
}
func (c *lspClient) requestContext(ctx context.Context, method string, params any, result any) error {
	c.mu.Lock()
	if c.terminal != nil {
		err := c.terminal
		c.mu.Unlock()
		return err
	}
	c.nextID++
	id := c.nextID
	ch := make(chan rpcEvent, 1)
	c.pending[id] = ch
	c.mu.Unlock()
	if err := c.send(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params}); err != nil {
		c.fail(err)
		return err
	}
	select {
	case <-ctx.Done():
		return ctx.Err() // the owner must retire the worker, never drain indefinitely
	case <-c.ctx.Done():
		return c.ctx.Err()
	case event := <-ch:
		if event.err != nil {
			return event.err
		}
		msg := event.message
		if msg.Error != nil {
			return fmt.Errorf("server error %d: %s", msg.Error.Code, msg.Error.Message)
		}
		if result != nil {
			return json.Unmarshal(msg.Result, result)
		}
		return nil
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
