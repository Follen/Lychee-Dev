package protocol_test

import (
	"encoding/json"
	"os/exec"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLuaRuntimeEpochSurvivesRebindingNotReload(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "runtime_epoch.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var result struct{ First, Second string }
	if err := json.Unmarshal(output, &result); err != nil {
		t.Fatal(err)
	}
	first, err := bridge.ParseSignal([]byte(result.First))
	if err != nil {
		t.Fatal(err)
	}
	second, err := bridge.ParseSignal([]byte(result.Second))
	if err != nil {
		t.Fatal(err)
	}
	if first.RuntimeEpoch != 1 || second.RuntimeEpoch != 2 || first.SessionNonce != second.SessionNonce || first.Sequence != second.Sequence {
		t.Fatalf("runtime and binding identities conflated: %+v %+v", first, second)
	}
	if err := first.Match(bridge.SignalExpectation{RuntimeEpoch: second.RuntimeEpoch}); err == nil {
		t.Fatal("old runtime matched new epoch")
	}
	if err := second.Match(bridge.SignalExpectation{RuntimeEpoch: second.RuntimeEpoch}); err != nil {
		t.Fatal(err)
	}
}
