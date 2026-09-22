package protocol_test

import (
	"encoding/json"
	"os/exec"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestLuaReportVerifiedByHost(t *testing.T) {
	path := luaRuntime(t)
	output, err := exec.Command(path, "report.lua", "../../addon/Bridge/CaptureWriter.lua").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var payload struct{ Receipt, Body, Code string }
	if err := json.Unmarshal(output, &payload); err != nil {
		t.Fatal(err)
	}
	expected := bridge.SignalExpectation{Kind: "reported", Release: "2.0.0", SessionNonce: "session", RequestID: "OP-protocol", Character: "character", Realm: "realm", Product: "retail", Build: "12.1.0.69875", AfterSequence: 3}
	report, err := bridge.VerifyReport([]byte(payload.Receipt), []byte(payload.Body), []byte(payload.Code), expected)
	if err != nil {
		t.Fatal(err)
	}
	if string(report.Body) != `{"answer":42,"complete":true,"text":"世界"}` {
		t.Fatalf("changed body: %s", report.Body)
	}
	if _, err := bridge.VerifyReport([]byte(payload.Receipt), append([]byte(payload.Body), ' '), []byte(payload.Code), expected); err == nil {
		t.Fatal("modified bytes accepted")
	}
}
