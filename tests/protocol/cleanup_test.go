package protocol_test

import (
	"encoding/json"
	"os/exec"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLuaRetirementConfirmation(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "cleanup.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var receipts []json.RawMessage
	if err := json.Unmarshal(output, &receipts); err != nil {
		t.Fatal(err)
	}
	if len(receipts) != 4 {
		t.Fatal("missing client profiles")
	}
	for _, raw := range receipts {
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			t.Fatal(err)
		}
		expected := bridge.SignalExpectation{Kind: "cleared", Release: "2.0.0-dev", SessionNonce: strings.Repeat("a", 32), RequestID: "OP-target", CleanupNonce: strings.Repeat("c", 32), Character: "Paladin", Realm: "Realm"}
		if err := signal.Match(expected); err != nil || signal.GUID != "Player-1-123" {
			t.Fatal("cleanup identity", err)
		}
		expected.CleanupNonce = strings.Repeat("d", 32)
		if signal.Match(expected) == nil {
			t.Fatal("wrong cleanup challenge accepted")
		}
	}
}
