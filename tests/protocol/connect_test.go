package protocol_test

import (
	"os/exec"
	"strings"
	"testing"
)

func TestPlayerConnectionFourClients(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "connect.lua", "../../addon", wowGlobals(t)).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	lines := strings.Split(strings.TrimSpace(string(output)), "\n")
	if len(lines) != 4 {
		t.Fatalf("expected four ready receipts: %s", output)
	}
	for i, product := range []string{"retail", "classic", "titan", "forever"} {
		// The ready receipt names its build but not the actor: the host fills the
		// actor from the session baseline before comparing.
		builds := []string{"12.1.0.12345", "5.5.4.12345", "3.80.2.12345", "1.60.1.12345"}
		signal := parseSessionSignal(t, []byte(strings.TrimSpace(lines[i])), sessionBaseline(product, builds[i]))
		if signal.Kind != "ready" || signal.Product != product || !signal.InputReady || signal.RuntimeEpoch != 1 || signal.Character != "Paladin" || len(signal.SessionNonce) != 32 || signal.RequestID != "" {
			t.Fatal(signal)
		}
	}
}
