package codebase

import (
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"os"
	"os/exec"
	"testing"
	"time"
)

// Opt-in cross-process acceptance uses the current compiled CLI and a complete
// verified release for its pinned LuaLS payload. It never connects to a game.
func TestRealBrokerAcrossCLIProcesses(t *testing.T) {
	binary := os.Getenv("LYCHEEDEV_BROKER_TEST_BINARY")
	release := os.Getenv("LYCHEEDEV_BROKER_TEST_RELEASE")
	if binary == "" || release == "" {
		t.Skip("compiled CLI and verified release not selected")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	b, seed := sourceFixture(t)
	docs := `local API={Name="Fixture",Type="System",Functions={{Name="FixtureAPI",Type="Function",Returns={{Name="v",Type="number"}}}}};APIDocumentation:AddDocumentationTable(API)`
	pin := testCommit(t, b, seed, map[string]string{"Interface/AddOns/Blizzard_APIDocumentationGenerated/Fixture.lua": docs, "probe.lua": "function First()\n return FixtureAPI()\nend\nfunction Second()\n return FixtureAPI()\nend\nfunction Third()\n return First()\nend\n"}, "broker fixed source")
	_, err := b.IndexSource(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	set, err := vault.WriteMetadata(ctx, b.store.Root(), func(_ *vault.Store, metadata *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
	})
	if err != nil {
		t.Fatal(err)
	}
	call := func(words ...string) map[string]any {
		t.Helper()
		args := append(words, "--home", b.store.Root(), "--format=json")
		if len(words) < 2 || words[1] != "prune" {
			args = append(args, "--release", release)
		}
		command := exec.CommandContext(ctx, binary, args...)
		raw, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("CLI %v: %v: %s", words, err, raw)
		}
		var envelope map[string]any
		if err = json.Unmarshal(raw, &envelope); err != nil {
			t.Fatalf("invalid envelope: %s", raw)
		}
		result, ok := envelope["result"].(map[string]any)
		if !ok {
			t.Fatalf("missing result: %s", raw)
		}
		return result
	}
	defer func() { call("source", "session", "close") }()
	status := call("source", "session", "status")
	if status["state"] != "closed" {
		t.Fatalf("status created broker: %v", status)
	}
	var workerPID any
	for i, name := range []string{"First", "Second", "Third"} {
		result := call("source", "refs", "--snapshot", set.ID, "--symbol", name, "--session-reuse", "--direction", "both")
		if result["complete"] != true {
			t.Fatalf("incomplete relation: %v", result)
		}
		status = call("source", "session", "status")
		if status["state"] != "ready" || status["batches"] != float64(i+1) {
			t.Fatalf("worker state: %v", status)
		}
		if i == 0 {
			t.Logf("first workerPid=%v", status["workerPid"])
		} else if status["workerPid"] != workerPID {
			t.Fatalf("worker was not reused: %v", status)
		}
		workerPID = status["workerPid"]
	}
	// The complete semantic cache hit does not perform another broker batch.
	_ = call("source", "refs", "--snapshot", set.ID, "--symbol", "First", "--session-reuse", "--direction", "both")
	status = call("source", "session", "status")
	if status["batches"] != float64(3) {
		t.Fatalf("cache hit ran semantic batch: %v", status)
	}
	pruned := call("source", "prune", "--target-bytes", "0")
	if pruned["complete"] != true {
		t.Fatalf("broker lease prevented prune: %v", pruned)
	}
	status = call("source", "session", "status")
	if status["state"] != "closed" {
		t.Fatalf("close did not retire: %v", status)
	}
	lease, err := b.AcquireWorktree(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
}
