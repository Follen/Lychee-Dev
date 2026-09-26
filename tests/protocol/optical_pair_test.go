package protocol_test

import (
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/bridge"
	"os/exec"
	"testing"
)

func TestOpticalPairRetainsProofsAndReducesArea(t *testing.T) {
	raw, err := exec.Command(luaRuntime(t), "optical_pair.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v: %s", err, raw)
	}
	var output struct {
		Receipt, Ready, Pair         string
		OldWidth, OldHeight, NewSide int
	}
	if err := json.Unmarshal(raw, &output); err != nil {
		t.Fatal(err)
	}
	signals, err := bridge.ParseOpticalSignals([]byte(output.Pair))
	if err != nil || len(signals) != 2 {
		t.Fatalf("pair: %+v %v", signals, err)
	}
	baseline := sessionBaseline("classic", "5.5.4.69934")
	baseline.SessionNonce = "26161b3b1bd87b850000000000000001"
	receipt := parseSessionSignal(t, []byte(output.Receipt), baseline)
	ready := parseSessionSignal(t, []byte(output.Ready), baseline)
	if bridge.FillSignalIdentity(signals[0], baseline) != receipt || bridge.FillSignalIdentity(signals[1], baseline) != ready {
		t.Fatal("transport changed independent proofs")
	}
	saved := 100 * (1 - float64(output.NewSide*output.NewSide)/float64(output.OldWidth*output.OldHeight))
	if saved < 20 {
		t.Fatalf("insufficient optical area reduction: %.1f%%", saved)
	}
	t.Logf("same 3 UI-unit modules: %dx%d -> %dx%d; area -%.1f%%; pair %d UTF-8 bytes", output.OldWidth, output.OldHeight, output.NewSide, output.NewSide, saved, len(output.Pair))
}
