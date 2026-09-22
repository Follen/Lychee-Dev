package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func invoke(t *testing.T, args ...string) (Envelope, int) {
	t.Helper()
	var out, log bytes.Buffer
	code := Execute(context.Background(), args, &out, &log)
	var result Envelope
	if err := json.Unmarshal(out.Bytes(), &result); err != nil {
		t.Fatalf("decode: %v: %s", err, out.String())
	}
	if log.Len() != 0 {
		t.Fatalf("unexpected stderr: %s", log.String())
	}
	if result.Schema != "lycheedev.result.v1" || result.Context == nil || result.Captures == nil || result.Warnings == nil {
		t.Fatalf("invalid envelope: %+v", result)
	}
	return result, code
}

func invokeExecutable(t *testing.T, executable string, args ...string) (Envelope, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, executable, args...)
	var out, log bytes.Buffer
	cmd.Stdout, cmd.Stderr = &out, &log
	err := cmd.Run()
	code := 0
	if err != nil {
		var failure *exec.ExitError
		if !errors.As(err, &failure) {
			t.Fatal(err)
		}
		code = failure.ExitCode()
	}
	var response Envelope
	if err := json.Unmarshal(out.Bytes(), &response); err != nil {
		t.Fatalf("%v: %s; %s", err, out.String(), log.String())
	}
	if log.Len() != 0 || response.Schema != "lycheedev.result.v1" || response.Context == nil || response.Captures == nil || response.Warnings == nil {
		t.Fatalf("invalid response: %+v stderr=%s", response, log.String())
	}
	return response, code
}

func TestMachineErrors(t *testing.T) {
	for _, args := range [][]string{
		{"missing", "--format", "json"},
		{"--invalid", "--format", "json"},
		{"version", "--format=json", "--home"},
		{"version", "--format=xml"},
		{"version", "--format=json", "--home=a", "--home=b"},
	} {
		result, code := invoke(t, args...)
		if code != 2 || result.OK || result.Error.Code != "command.invalid_arguments" {
			t.Fatalf("%v: %+v code %d", args, result, code)
		}
	}
}

func TestWorkspaceSelectionAndLegacyIsolation(t *testing.T) {
	base := t.TempDir()
	environmentRoot := filepath.Join(base, "environment")
	explicitRoot := filepath.Join(base, "explicit")
	t.Setenv("LYCHEEDEV_HOME", environmentRoot)
	result, code := invoke(t, "init", "--home", explicitRoot, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("%+v code %d", result, code)
	}
	if _, err := os.Stat(environmentRoot); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("environment root touched: %v", err)
	}
	result, code = invoke(t, "init", "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("environment init: %+v code %d", result, code)
	}
	legacy := filepath.Join(base, "legacy")
	if err := os.Mkdir(legacy, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(legacy, "config.json")
	if err := os.WriteFile(sentinel, []byte("old user data"), 0600); err != nil {
		t.Fatal(err)
	}
	result, code = invoke(t, "init", "--home", legacy, "--format=json")
	if code != 3 || result.OK || result.Error.Code != "vault.legacy_detected" {
		t.Fatalf("legacy: %+v code %d", result, code)
	}
	data, err := os.ReadFile(sentinel)
	if err != nil || string(data) != "old user data" {
		t.Fatalf("legacy changed: %q %v", data, err)
	}
}

func TestReadOnlyCommandsDoNotCreateHome(t *testing.T) {
	root := filepath.Join(t.TempDir(), "absent")
	t.Setenv("LYCHEEDEV_HOME", root)
	for _, name := range []string{"version", "describe", "--help"} {
		result, code := invoke(t, name, "--format=json")
		if code != 0 || !result.OK {
			t.Fatalf("%s: %+v", name, result)
		}
	}
	if response, code := invoke(t, "source", "list", "--format=json"); code != 0 || !response.OK {
		t.Fatalf("source list: %+v", response)
	}
	if _, err := os.Stat(root); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("read-only commands touched workspace: %v", err)
	}
}

func TestSourceFlagsAreScopedAndRequired(t *testing.T) {
	for _, args := range [][]string{
		{"source", "sync"}, {"source", "sync", "--source", "wow-ui-source"},
		{"source", "diff", "--from", "PIN-a"},
		{"source", "diff", "--from", "PIN-a", "--to", "PIN-b", "--limit", "201"},
		{"version", "--from", "PIN-a"},
		{"source", "diff", "--snapshot", "PIN-a"},
		{"source", "validate", "--snapshot", "PIN-a", "--path", "addon"},
		{"source", "query", "Name", "--toc", "Addon.toc"},
		{"version", "--path", "addon"},
		{"source", "inspect", "--path", "a.lua"},
		{"source", "inspect", "--snapshot", "PIN-a", "--path", "a.lua", "--count", "2001"},
		{"version", "--snapshot", "PIN-a"}, {"source", "list", "--product", "retail"},
		{"source", "sync", "--source", "wow-ui-source", "--source", "ndui"},
	} {
		response, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || response.OK {
			t.Fatalf("%v: %d %+v", args, code, response)
		}
	}
}

func TestJSONLFraming(t *testing.T) {
	for _, test := range []struct {
		command string
		types   []string
		code    int
	}{
		{"version", []string{"begin", "record", "end"}, 0},
		{"missing", []string{"begin", "error"}, 2},
	} {
		var out, log bytes.Buffer
		if code := Execute(context.Background(), []string{test.command, "--format=jsonl"}, &out, &log); code != test.code {
			t.Fatalf("exit %d", code)
		}
		decoder := json.NewDecoder(&out)
		for _, expected := range test.types {
			var frame map[string]any
			if err := decoder.Decode(&frame); err != nil {
				t.Fatal(err)
			}
			if frame["type"] != expected {
				t.Fatalf("expected %s got %v", expected, frame)
			}
		}
		var extra any
		if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
			t.Fatalf("extra frame %v %v", extra, err)
		}
	}
}

type brokenOutput struct{}

func (brokenOutput) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func TestBrokenOutputDoesNotSucceed(t *testing.T) {
	var log bytes.Buffer
	if code := Execute(context.Background(), []string{"version", "--format=jsonl"}, brokenOutput{}, &log); code != 5 {
		t.Fatalf("exit %d", code)
	}
	if log.Len() == 0 {
		t.Fatal("missing output diagnostic")
	}
}
