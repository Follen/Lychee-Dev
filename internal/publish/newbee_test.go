package publish

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewBeeOfficialPlanNeverDiscoversOrAuthenticates(t *testing.T) {
	home := filepath.Join(t.TempDir(), "absent")
	result, err := RunNewBee(context.Background(), "official session auth", Request{Home: home, Plan: true, Input: []byte(`{"cli_path":"missing-secret-path"}`)})
	if err != nil || !result.Complete {
		t.Fatalf("%+v %v", result, err)
	}
	if _, err = os.Stat(home); !os.IsNotExist(err) {
		t.Fatalf("plan created home: %v", err)
	}
	for _, input := range []string{`{"id":1,"id":2}`, `{"id":null}`, `{"id":1,"token":"secret"}`} {
		_, err = RunNewBee(context.Background(), "official plugin get", Request{Plan: true, Input: []byte(input)})
		if err == nil {
			t.Fatalf("accepted %s", input)
		}
	}
	_, err = RunNewBee(context.Background(), "official plugin sync", Request{Plan: true, RequestID: "r", Input: []byte(`{"directory":"x","yes":false}`)})
	if err == nil {
		t.Fatal("accepted destructive false sync")
	}
}

func TestNewBeeOfficialMutationReceiptAndChangedFile(t *testing.T) {
	dir := t.TempDir()
	cli := filepath.Join(dir, "ncc.cjs")
	count := filepath.Join(dir, "calls.txt")
	file := filepath.Join(dir, "wa.txt")
	script := `const fs=require('fs');fs.appendFileSync(process.env.NBB_TEST_COUNT,'x');console.log(JSON.stringify({id:7,token:'SECRET',wa_str:'RAW',content:'CONFIG',url:'https://cdn.example/a?signature=SECRET',args:process.argv.slice(2)}));`
	if err := os.WriteFile(cli, []byte(script), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("NBB_TEST_COUNT", count)
	if err := os.WriteFile(file, []byte("one"), 0600); err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(map[string]any{"cli_path": cli, "id": 7, "file": file, "log": "fix"})
	request := Request{Home: dir, RequestID: "original", Input: raw}
	first, err := RunNewBee(context.Background(), "official wa upload", request)
	if err != nil || !first.Complete {
		t.Fatalf("%+v %v", first, err)
	}
	encoded, _ := json.Marshal(first)
	if strings.Contains(string(encoded), "SECRET") || strings.Contains(string(encoded), "RAW") || strings.Contains(string(encoded), "CONFIG") {
		t.Fatalf("leaked content: %s", encoded)
	}
	second, err := RunNewBee(context.Background(), "official wa upload", request)
	if err != nil || second.OperationID != first.OperationID {
		t.Fatalf("%+v %v", second, err)
	}
	calls, _ := os.ReadFile(count)
	if string(calls) != "x" {
		t.Fatalf("replayed: %s", calls)
	}
	if err = os.WriteFile(file, []byte("two"), 0600); err != nil {
		t.Fatal(err)
	}
	_, err = RunNewBee(context.Background(), "official wa upload", request)
	var pe *Error
	if !errors.As(err, &pe) || pe.Exit != 2 {
		t.Fatalf("changed file accepted: %v", err)
	}
}

func TestNewBeeOfficialUncertainNeverReplays(t *testing.T) {
	dir := t.TempDir()
	cli := filepath.Join(dir, "ncc.cjs")
	count := filepath.Join(dir, "calls.txt")
	script := `const fs=require('fs');fs.appendFileSync(process.env.NBB_TEST_COUNT,'x');process.exit(3);`
	if err := os.WriteFile(cli, []byte(script), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("NBB_TEST_COUNT", count)
	raw, _ := json.Marshal(map[string]any{"cli_path": cli, "id": 7, "set": "fix"})
	request := Request{Home: dir, RequestID: "original", Input: raw}
	if _, err := RunNewBee(context.Background(), "official plugin changelog-edit", request); err == nil {
		t.Fatal("expected subprocess failure")
	}
	result, err := RunNewBee(context.Background(), "official plugin changelog-edit", request)
	var pe *Error
	if !errors.As(err, &pe) || pe.Exit != 6 || result.Submission != "uncertain" {
		t.Fatalf("%+v %v", result, err)
	}
	calls, _ := os.ReadFile(count)
	if string(calls) != "x" {
		t.Fatalf("replayed: %s", calls)
	}
}

func TestNewBeeOfficialArgumentsPreserveExplicitEmpty(t *testing.T) {
	c := newBeeOfficialCommands["plugin edit"]
	input, err := c.spec.Decode([]byte(`{"id":8,"intro":"","private":false,"subscribe_level":0}`))
	if err != nil {
		t.Fatal(err)
	}
	args, err := newBeeArgs(c, input)
	if err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(args)
	if string(encoded) != `["wow","addons","edit","8","--intro","","--subscribe-level","0","-o","json"]` {
		t.Fatalf("%s", encoded)
	}
}

func TestNewBeeContractsHaveStrictValidExamples(t *testing.T) {
	actions := NewBeeActions()
	t.Logf("NewBee registered actions: %d", len(actions))
	seen := map[string]bool{}
	for _, action := range actions {
		if seen[action] {
			t.Fatalf("duplicate %s", action)
		}
		seen[action] = true
		spec, ok := NewBeeInput(action)
		if !ok {
			t.Fatalf("missing spec %s", action)
		}
		raw, err := json.Marshal(spec.Example)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = spec.Decode(raw); err != nil {
			t.Fatalf("%s invalid example %s: %v", action, raw, err)
		}
	}
}
