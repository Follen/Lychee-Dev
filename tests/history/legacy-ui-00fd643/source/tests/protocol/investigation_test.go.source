package protocol_test

import (
	"encoding/json"
	"os/exec"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestInvestigationOwnsResourcesThroughFinalization(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "investigation.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var result struct{ Observed, Finished string }
	if err := json.Unmarshal(output, &result); err != nil {
		t.Fatal(err, string(output))
	}
	for _, test := range []struct{ raw, state, nonce string }{{result.Observed, "reported", "c"}, {result.Finished, "finished", "e"}} {
		signal, err := bridge.ParseSignal([]byte(test.raw))
		if err != nil {
			t.Fatal(err, test.raw)
		}
		if signal.WorkState != test.state || !signal.ResourcesReleased || signal.CodeSHA256 != strings.Repeat("b", 64) {
			t.Fatalf("%+v", signal)
		}
		if err := signal.Match(bridge.SignalExpectation{Kind: "checkpoint", RequestID: "REQ-investigation", ProbeNonce: strings.Repeat(test.nonce, 32), SessionNonce: strings.Repeat("a", 32), RuntimeEpoch: 1}); err != nil {
			t.Fatal(err)
		}
		if err := signal.Match(bridge.SignalExpectation{Kind: "checkpoint", ProbeNonce: strings.Repeat("0", 32)}); err == nil {
			t.Fatal("old challenge accepted")
		}
	}
}
