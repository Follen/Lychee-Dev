package protocol_test

import (
	"encoding/json"
	"os/exec"
	"testing"

	"github.com/follenfang/lycheedev/internal/buildinfo"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestFourClientProbeExecution(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "probe.lua", "../../addon", wowGlobals(t)).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var results []struct{ Loaded, Receipt, Body, Code string }
	if err := json.Unmarshal(output, &results); err != nil {
		t.Fatal(err)
	}
	if len(results) != 4 {
		t.Fatalf("expected four clients, got %d", len(results))
	}
	products := []string{"retail", "classic", "titan", "forever"}
	builds := []string{"12.1.0.12345", "5.5.4.12345", "3.80.2.12345", "1.60.1.12345"}
	for i, result := range results {
		expected := bridge.SignalExpectation{Release: buildinfo.Version, Kind: "loaded", SessionNonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", RequestID: "OP-main", Character: "Paladin", Realm: "Realm", Product: products[i], Build: builds[i]}
		loaded := parseSessionSignal(t, []byte(result.Loaded), sessionBaseline(products[i], builds[i]))
		if err := loaded.Match(expected); err != nil {
			t.Fatal(err)
		}
		if loaded.InputReady || loaded.CodeBytes != uint32(len(result.Code)) {
			t.Fatalf("invalid load receipt: %+v", loaded)
		}
		expected.Kind, expected.AfterSequence = "reported", loaded.Sequence
		// VerifyReport parses the receipt from the wire and has no session
		// baseline, so it verifies the digests and the expectation the caller
		// supplies rather than the compacted actor identity.
		report, err := bridge.VerifyReport([]byte(result.Receipt), []byte(result.Body), []byte(result.Code), expected)
		if err != nil {
			t.Fatal(err)
		}
		if string(report.Body) != `{"probeStatus":"completed","result":{"answer":42}}` {
			t.Fatalf("unexpected body: %s", report.Body)
		}
		if loaded.CodeAdler32 != report.Receipt.CodeAdler32 {
			t.Fatal("loaded code differs from executed code")
		}
	}
}
