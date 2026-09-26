package protocol_test

import (
	"encoding/json"
	"os/exec"
	"strings"
	"testing"
)

func TestFourClientReloadReentry(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "reentry.lua", "../../addon", wowGlobals(t)).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var receipts []string
	if err := json.Unmarshal(output, &receipts); err != nil {
		t.Fatal(err)
	}
	if len(receipts) != 12 {
		t.Fatalf("missing client receipts: %d", len(receipts))
	}
	for _, text := range receipts {
		// Every reentry and cleanup receipt omits the actor identity; the
		// retained session baseline supplies it before comparison.
		baseline := sessionBaseline("retail", "12.1.0.12345")
		signal := parseSessionSignal(t, []byte(text), baseline)
		if signal.RequestID != "Reload-A" || signal.GUID != "Player-1-123" || signal.ReportBytes != 0 {
			t.Fatalf("invalid reentry: %+v", signal)
		}
		if signal.Kind == "cleared" {
			if signal.RuntimeEpoch != 3 || signal.CleanupNonce != strings.Repeat("e", 32) || signal.InputReady || signal.ReloadNonce != "" {
				t.Fatalf("invalid cleanup: %+v", signal)
			}
		} else if signal.RuntimeEpoch != 2 || signal.Kind != "ready" || signal.ReloadNonce != strings.Repeat("b", 32) || !signal.InputReady {
			t.Fatalf("invalid report reentry: %+v", signal)
		}
	}
}
