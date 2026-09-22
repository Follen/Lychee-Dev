package protocol_test

import (
	"os/exec"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestPlayerConnectionFourClients(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "connect.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	lines := strings.Split(strings.TrimSpace(string(output)), "\n")
	if len(lines) != 4 {
		t.Fatalf("expected four ready receipts: %s", output)
	}
	for i, product := range []string{"retail", "classic", "titan", "forever"} {
		signal, err := bridge.ParseSignal([]byte(strings.TrimSpace(lines[i])))
		if err != nil || signal.Kind != "ready" || signal.Product != product || !signal.InputReady || signal.RuntimeEpoch != 1 || signal.Character != "Paladin" || len(signal.SessionNonce) != 32 || signal.RequestID != "" {
			t.Fatal(signal, err)
		}
	}
}
