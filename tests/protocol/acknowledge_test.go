package protocol_test

import (
	"encoding/json"
	"os/exec"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLuaAcknowledgementAfterSessionRecreation(t *testing.T) {
	output, err := exec.Command(luaRuntime(t), "acknowledge.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var payload struct{ Receipt, Body, Code, Acknowledgement string }
	if err := json.Unmarshal(output, &payload); err != nil {
		t.Fatal(err)
	}
	expected := bridge.SignalExpectation{Release: "2.0.0-dev", Kind: "reported", SessionNonce: strings.Repeat("a", 32), RequestID: "OP-ack", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.69875", AfterSequence: 101}
	report, err := bridge.VerifyReport([]byte(payload.Receipt), []byte(payload.Body), []byte(payload.Code), expected)
	if err != nil {
		t.Fatal(err)
	}
	acknowledgement, err := bridge.ParseSignal([]byte(payload.Acknowledgement))
	if err != nil {
		t.Fatal(err)
	}
	expected.Kind, expected.AfterSequence = "acknowledged", report.Receipt.Sequence
	if err := acknowledgement.Match(expected); err != nil {
		t.Fatal(err)
	}
	if acknowledgement.InputReady || acknowledgement.CodeBytes != report.Receipt.CodeBytes || acknowledgement.CodeAdler32 != report.Receipt.CodeAdler32 || acknowledgement.ReportBytes != report.Receipt.ReportBytes || acknowledgement.ReportAdler32 != report.Receipt.ReportAdler32 {
		t.Fatalf("acknowledgement payload changed: %+v", acknowledgement)
	}
	if acknowledgement.Sequence != 104 {
		t.Fatalf("failed encoding reused sequence: %d", acknowledgement.Sequence)
	}
}
