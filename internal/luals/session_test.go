package luals

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestLSPPumpAnswersRequestsWhileIdle(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()
	c := newLSPClient(ctx, client, client)
	serverClient := &lspClient{input: server, pending: map[int]chan rpcEvent{}}
	sent := make(chan error, 1)
	go func() {
		sent <- serverClient.send(map[string]any{"jsonrpc": "2.0", "id": 77, "method": "window/workDoneProgress/create", "params": map[string]any{"token": "fixture"}})
	}()
	reader := bufio.NewReader(server)
	body, err := readFrame(reader)
	if err != nil {
		t.Fatal(err)
	}
	var response rpcMessage
	if err = json.Unmarshal(body, &response); err != nil || string(response.ID) != "77" || len(response.Result) == 0 {
		t.Fatalf("response=%s err=%v", body, err)
	}
	if err = <-sent; err != nil {
		t.Fatal(err)
	}
	// An idle notification is consumed rather than occupying the response slot.
	go func() {
		sent <- serverClient.notify("textDocument/publishDiagnostics", map[string]any{"uri": "fixture", "diagnostics": []any{}})
	}()
	if err = <-sent; err != nil {
		t.Fatal(err)
	}
	if err = c.beginBatch(); err != nil {
		t.Fatal(err)
	}
	reply := make(chan error, 1)
	go func() {
		body, e := readFrame(reader)
		if e != nil {
			reply <- e
			return
		}
		var req rpcMessage
		e = json.Unmarshal(body, &req)
		if e == nil {
			e = serverClient.send(map[string]any{"jsonrpc": "2.0", "id": req.ID, "result": []any{}})
		}
		reply <- e
	}()
	var result json.RawMessage
	if err = c.request("textDocument/definition", map[string]any{}, &result); err != nil {
		t.Fatal(err)
	}
	if err = <-reply; err != nil {
		t.Fatal(err)
	}
}
func TestLSPPumpRejectsUnexpectedIDAndLifetimeFlood(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	read, write := io.Pipe()
	defer read.Close()
	defer write.Close()
	c := newLSPClient(ctx, io.Discard, read)
	// Keep the test small while exercising the actual accounting boundary.
	c.mu.Lock()
	c.lifetimeBytes = (256 << 20) - 1
	c.mu.Unlock()
	_, _ = write.Write([]byte("Content-Length: 36\r\n\r\n{\"jsonrpc\":\"2.0\",\"method\":\"fixture\"}"))
	select {
	case <-c.done:
	case <-ctx.Done():
		t.Fatal("pump did not retire after lifetime budget")
	}
	if err := c.beginBatch(); err == nil {
		t.Fatal("accepted retired transport")
	}
}
func TestRealWorkspaceSessionWarmAndCancellation(t *testing.T) {
	dir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if dir == "" {
		t.Skip("real LuaLS runtime not selected")
	}
	r := &Runtime{directory: dir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "a.lua"), []byte("local answer = 42\nreturn answer\n"), 0600); err != nil {
		t.Fatal(err)
	}
	session, err := r.OpenBrokerWorkspaceSession(ctx, workspace, nil, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	query := []Query{{Kind: Definition, Path: "a.lua", Position: Position{Line: 1, Character: 7}}}
	first, err := session.Analyze(ctx, query)
	if err != nil {
		t.Fatal(err)
	}
	pid := session.PID()
	second, err := session.Analyze(ctx, query)
	if err != nil {
		t.Fatal(err)
	}
	firstRaw, _ := json.Marshal(first.Results)
	secondRaw, _ := json.Marshal(second.Results)
	if string(firstRaw) != string(secondRaw) || session.PID() != pid {
		t.Fatalf("warm changed results/owner: %s / %s", firstRaw, secondRaw)
	}
	cancelled, stop := context.WithCancel(ctx)
	stop()
	if _, err = session.Analyze(cancelled, query); err == nil {
		t.Fatal("cancelled request succeeded")
	}
	if _, err = session.Analyze(ctx, query); err == nil {
		t.Fatal("cancelled worker reused")
	}
	fresh, err := r.OpenBrokerWorkspaceSession(ctx, workspace, nil, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer fresh.Close()
	if _, err = fresh.Analyze(ctx, query); err != nil {
		t.Fatal(err)
	}
}
func TestRealSessionCancelsAnInflightReadinessRequest(t *testing.T) {
	dir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if dir == "" {
		t.Skip("real LuaLS runtime not selected")
	}
	r := &Runtime{directory: dir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	workspace := t.TempDir()
	var source strings.Builder
	for i := 0; i < 10000; i++ {
		fmt.Fprintf(&source, "function Value%05d() return %d end\n", i, i)
	}
	if err := os.WriteFile(filepath.Join(workspace, "a.lua"), []byte(source.String()), 0600); err != nil {
		t.Fatal(err)
	}
	session, err := r.OpenBrokerWorkspaceSession(ctx, workspace, nil, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	active, stop := context.WithCancel(ctx)
	defer stop()
	done := make(chan error, 1)
	go func() {
		_, err := session.Analyze(active, []Query{{Kind: Definition, Path: "a.lua", Position: Position{Line: 9999, Character: 10}}})
		done <- err
	}()
	// Wait only until the owned RPC transport has actually admitted a request.
	for {
		session.client.mu.Lock()
		pending := len(session.client.pending) > 0
		session.client.mu.Unlock()
		if pending {
			break
		}
		select {
		case err := <-done:
			t.Fatalf("request ended before active cancellation: %v", err)
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		case <-time.After(time.Millisecond):
		}
	}
	stop()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("active cancellation: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("active cancellation did not retire bounded worker")
	}
	if session.Usable() {
		t.Fatal("cancelled readiness worker remains reusable")
	}
}
func TestZeroQueryWorkspaceDoesNotStartSemanticProcess(t *testing.T) {
	r := &Runtime{Identity: Pinned()}
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "a.lua"), []byte("return 1\n"), 0600); err != nil {
		t.Fatal(err)
	}
	// No runtime executable exists. An empty query must inspect coverage only.
	result, err := r.AnalyzeWorkspace(context.Background(), workspace, nil, nil)
	if err != nil || result.State != "complete" || result.Coverage.LuaFiles != 1 {
		t.Fatalf("empty query started runtime or lost coverage: %+v %v", result, err)
	}
}
func TestWorkspaceBatchInputValidatedBeforeStartup(t *testing.T) {
	r := &Runtime{Identity: Pinned()}
	if _, err := r.AnalyzeWorkspaceBatches(context.Background(), t.TempDir(), nil, [][]Query{make([]Query, 129)}); !errors.Is(err, ErrBudget) {
		t.Fatalf("invalid batch reached startup: %v", err)
	}
}
func TestLSPBatchAndLifetimeBudgetsAreIndependent(t *testing.T) {
	// Resetting the per-batch allowance preserves every lifetime byte.
	c := &lspClient{pending: map[int]chan rpcEvent{}}
	if err := c.account(32 << 20); err != nil {
		t.Fatal(err)
	}
	if err := c.account(1); !errors.Is(err, ErrBudget) {
		t.Fatalf("per-batch budget: %v", err)
	}
	if err := c.beginBatch(); err != nil {
		t.Fatal(err)
	}
	if err := c.account(1); err != nil {
		t.Fatal(err)
	}
	if c.lifetimeBytes != (32<<20)+2 {
		t.Fatalf("reset lifetime budget: %d", c.lifetimeBytes)
	}
}
func TestRealSessionEnvironmentAndConfigIsolation(t *testing.T) {
	dir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if dir == "" {
		t.Skip("real LuaLS runtime not selected")
	}
	r := &Runtime{directory: dir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "a.lua"), []byte("local x = FixtureAPI()\nreturn x\n"), 0600); err != nil {
		t.Fatal(err)
	}
	// Source configuration cannot insert a plugin or unauthorized external library.
	if err := os.WriteFile(filepath.Join(workspace, ".luarc.json"), []byte(`{"runtime.plugin":"missing-plugin.lua","workspace.library":["C:/untrusted"]}`), 0600); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"number", "string"} {
		definitions := []byte("---@return " + kind + "\nfunction FixtureAPI() end\n")
		session, err := r.OpenBrokerWorkspaceSession(ctx, workspace, definitions, t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		for batch := 0; batch < 2; batch++ {
			result, err := session.Analyze(ctx, []Query{{Kind: Hover, Path: "a.lua", Position: Position{Line: 1, Character: 7}}})
			if err != nil {
				session.Close()
				t.Fatal(err)
			}
			if len(result.Results) != 1 || !strings.Contains(result.Results[0].Hover, kind) {
				session.Close()
				t.Fatalf("%s environment batch%d: %+v", kind, batch, result)
			}
		}
		// Restoring the exact bytes cannot restore an old in-memory session.
		if err := os.WriteFile(filepath.Join(workspace, ".luarc.json"), []byte(`{}`), 0600); err != nil {
			t.Fatal(err)
		}
		select {
		case <-session.client.done:
		case <-time.After(3 * time.Second):
			session.Close()
			t.Fatal("source config edit did not retire worker")
		}
		session.Close()
		if err := os.WriteFile(filepath.Join(workspace, ".luarc.json"), []byte(`{"runtime.plugin":"missing-plugin.lua","workspace.library":["C:/untrusted"]}`), 0600); err != nil {
			t.Fatal(err)
		}
	}
}
func TestLSPPumpRejectsUnassociatedResponseID(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	reader, writer := io.Pipe()
	defer reader.Close()
	defer writer.Close()
	client := newLSPClient(ctx, io.Discard, reader)
	server := &lspClient{input: writer, pending: map[int]chan rpcEvent{}}
	if err := server.send(map[string]any{"jsonrpc": "2.0", "id": 9, "result": nil}); err != nil {
		t.Fatal(err)
	}
	select {
	case <-client.done:
	case <-ctx.Done():
		t.Fatal("unassociated response not rejected")
	}
	if err := client.beginBatch(); err == nil {
		t.Fatal("reused transport after wrong response ID")
	}
}
func TestLSPHeaderWithoutNewlineIsBounded(t *testing.T) {
	if _, err := readFrame(bufio.NewReader(strings.NewReader(strings.Repeat("x", 100000)))); !errors.Is(err, ErrBudget) {
		t.Fatalf("unbounded header: %v", err)
	}
}
