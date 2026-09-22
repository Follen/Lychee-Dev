package protocol_test

import (
	"encoding/json"
	"os/exec"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLoadedReadinessRefresh(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "loaded_ready.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var result struct{ Initial, Refreshed, Reported, ReportReady, ReadyInitial, SessionReady string }
	if err := json.Unmarshal(output, &result); err != nil {
		t.Fatal(err)
	}
	initial, err := bridge.ParseSignal([]byte(result.Initial))
	if err != nil {
		t.Fatal(err)
	}
	refreshed, err := bridge.ParseSignal([]byte(result.Refreshed))
	if err != nil {
		t.Fatal(err)
	}
	reported, err := bridge.ParseSignal([]byte(result.Reported))
	if err != nil {
		t.Fatal(err)
	}
	if initial.InputReady || !refreshed.InputReady || refreshed.Sequence <= initial.Sequence || reported.Sequence <= refreshed.Sequence {
		t.Fatal("readiness/sequence mismatch")
	}
	copy := refreshed
	copy.InputReady, copy.Sequence = initial.InputReady, initial.Sequence
	if copy != initial || reported.Kind != "reported" {
		t.Fatal("refresh changed loaded request identity")
	}
	reportReady, err := bridge.ParseSignal([]byte(result.ReportReady))
	if err != nil || reportReady.Kind != "ready" || !reportReady.InputReady || reportReady.RequestID != "" || reportReady.Sequence <= reported.Sequence || reportReady.GUID != "Player-1-123" {
		t.Fatalf("invalid post-report readiness: %+v %v", reportReady, err)
	}
	readyInitial, err := bridge.ParseSignal([]byte(result.ReadyInitial))
	if err != nil {
		t.Fatal(err)
	}
	ready, err := bridge.ParseSignal([]byte(result.SessionReady))
	if err != nil {
		t.Fatal(err)
	}
	if ready.Kind != "ready" || ready.RequestID != "" || ready.GUID != "Player-1-123" || readyInitial.InputReady || !ready.InputReady || ready.Sequence <= readyInitial.Sequence {
		t.Fatalf("invalid session readiness: %+v %+v", readyInitial, ready)
	}
}
