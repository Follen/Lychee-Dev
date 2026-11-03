//go:build windows && amd64

package publish

import (
	"context"
	"io"
	"net/url"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

type ddRetainedFixture struct{ calls int }

func (a *ddRetainedFixture) identity() map[string]any {
	return map[string]any{"author_id": "synthetic-author", "source": "fixture", "active": true}
}
func (a *ddRetainedFixture) call(ctx context.Context, path string, q url.Values, body any, mutation bool) (map[string]any, error) {
	if path == "/native-hang" {
		select {}
	}
	a.calls++
	return map[string]any{"code": 200, "result": map[string]any{"calls": a.calls, "mutation": mutation}}, nil
}

func TestDDSessionWorkerNativeDeadlineEndsOriginalProcess(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	home := t.TempDir()
	id := "DD-" + ddRandom()
	state := ddSessionState{Schema: ddSessionWire, ID: id, Nonce: ddRandom(), Pipe: ddPipeName(id), Status: "starting"}
	if err := ddSaveSession(ctx, home, state); err != nil {
		t.Fatal(err)
	}
	exe, _ := os.Executable()
	cmd := exec.Command(exe, "-test.run=^TestDDSessionWorkerHelper$")
	cmd.Env = append(os.Environ(), "LYCHEEDEV_TEST_DD_SESSION_WORKER=1")
	ddConfigureWorker(cmd)
	in, _ := cmd.StdinPipe()
	out, _ := cmd.StdoutPipe()
	cmd.Stderr = io.Discard
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer cmd.Process.Kill()
	defer out.Close()
	ddWriteFrame(in, ddSessionOpen{Home: home, ID: id, Nonce: state.Nonce, Installation: "synthetic", Deadline: time.Now().Add(5 * time.Second).UnixMilli()})
	in.Close()
	var ready ddSessionReply
	if err := ddReadFrame(out, &ready); err != nil || ready.Error != nil {
		t.Fatalf("startup %v", err)
	}
	api, err := attachDDSession(ctx, home, id)
	if err != nil {
		t.Fatal(err)
	}
	operation, finish := context.WithTimeout(ctx, 30*time.Millisecond)
	defer finish()
	if _, err = api.call(operation, "/native-hang", nil, nil, true); err == nil {
		t.Fatal("hung request completed")
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case err = <-done:
		if exit, ok := err.(*exec.ExitError); !ok || exit.ExitCode() != 5 {
			t.Fatalf("watchdog exit %v", err)
		}
	case <-ctx.Done():
		t.Fatal("native deadline did not retire original process")
	}
	result, err := runDDSession(ctx, "session close", Request{Home: home}, map[string]any{"session": id})
	if err != nil || !result.Complete {
		t.Fatalf("dead worker cleanup %v", err)
	}
}
func (a *ddRetainedFixture) channels(context.Context) (any, error) {
	return map[string]any{"items": []any{}}, nil
}
func (a *ddRetainedFixture) close() error { return nil }
func TestDDSessionWorkerHelper(t *testing.T) {
	if os.Getenv("LYCHEEDEV_TEST_DD_SESSION_WORKER") != "1" {
		t.Skip("subprocess fixture")
	}
	if err := ddSessionWorker(context.Background(), os.Stdin, os.Stdout, func(context.Context, string, string) (ddAPI, error) { return &ddRetainedFixture{}, nil }); err != nil {
		os.Exit(1)
	}
}
func TestDDSessionRetainedWorkerAcrossAttachmentsAndDeath(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	home := t.TempDir()
	id := "DD-" + ddRandom()
	state := ddSessionState{Schema: ddSessionWire, ID: id, Nonce: ddRandom(), Pipe: ddPipeName(id), Status: "starting"}
	if err := ddSaveSession(ctx, home, state); err != nil {
		t.Fatal(err)
	}
	executable, _ := os.Executable()
	cmd := exec.Command(executable, "-test.run=^TestDDSessionWorkerHelper$")
	cmd.Env = append(os.Environ(), "LYCHEEDEV_TEST_DD_SESSION_WORKER=1")
	ddConfigureWorker(cmd)
	input, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	output, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	cmd.Stderr = io.Discard
	if err = cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { cmd.Process.Kill(); cmd.Wait(); output.Close() }()
	ddWriteFrame(input, ddSessionOpen{Home: home, ID: id, Nonce: state.Nonce, Installation: "synthetic", Deadline: time.Now().Add(5 * time.Second).UnixMilli()})
	input.Close()
	var ready ddSessionReply
	if err = ddReadFrame(output, &ready); err != nil || ready.Error != nil {
		t.Fatalf("startup %v %#v", err, ready.Error)
	}
	for i := 1; i <= 2; i++ {
		api, e := attachDDSession(ctx, home, id)
		if e != nil {
			t.Fatal(e)
		}
		value, e := api.call(ctx, "/fixture", nil, nil, true)
		api.close()
		if e != nil {
			t.Fatal(e)
		}
		result := value["result"].(map[string]any)
		if ddInteger(result["calls"]) != int64(i) {
			t.Fatal("attachment replaced runtime")
		}
	}
	record, e := ddLoadSession(home, id)
	if e != nil {
		t.Fatal(e)
	}
	wrong := record
	wrong.Created++
	if _, e = ddDialSession(ctx, wrong); e == nil {
		t.Fatal("accepted process creation mismatch")
	}
	wrong = record
	wrong.Nonce = strings.Repeat("0", 48)
	if _, e = (&ddSessionClient{state: wrong}).exchange(ctx, ddSessionCall{Operation: "call", Path: "/fixture", Mutation: true}); e == nil {
		t.Fatal("accepted wrong capability")
	}
	result, e := runDDSession(ctx, "session close", Request{Home: home}, map[string]any{"session": id})
	if e != nil || !result.Complete || result.PendingCleanup {
		t.Fatalf("close %v %#v", e, result)
	}
	if _, e = attachDDSession(ctx, home, id); e == nil {
		t.Fatal("closed runtime attached")
	}
	again, e := runDDSession(ctx, "session close", Request{Home: home}, map[string]any{"session": id})
	if e != nil || !again.Complete {
		t.Fatal("repeat close")
	}
}
func TestDDDeadWorkerClosePreservesRecordAndNeverStartsLogin(t *testing.T) {
	ctx := context.Background()
	home := t.TempDir()
	state := ddSessionState{Schema: ddSessionWire, ID: "DD-" + ddRandom(), Nonce: ddRandom(), Status: "starting"}
	state.Pipe = ddPipeName(state.ID)
	if err := ddSaveSession(ctx, home, state); err != nil {
		t.Fatal(err)
	}
	result, err := runDDSession(ctx, "session close", Request{Home: home}, map[string]any{"session": state.ID})
	if err != nil || !result.Complete {
		t.Fatalf("%v %#v", err, result)
	}
	saved, err := ddLoadSession(home, state.ID)
	if err != nil || saved.Status != "closed" {
		t.Fatal("original session not preserved")
	}
}
