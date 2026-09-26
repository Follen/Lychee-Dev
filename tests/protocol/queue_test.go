package protocol_test

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/buildinfo"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestHostQueueToLuaExecution(t *testing.T) {
	code := "queueExecuted = 1; return { text = '世界', escaped = '\"\\\\' } -- ]] injected delimiters"
	d := bridge.ProbeDefinition{RequestID: "OP-target", Release: buildinfo.Version, SessionNonce: strings.Repeat("a", 32), ReloadNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Code: code}
	other := d
	other.RequestID = "OP-other"
	other.Character = "Other"
	other.Code = "error('another agent must not execute')"
	data, err := bridge.EncodeProbeQueue([]bridge.ProbeDefinition{other, d})
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "Definitions.lua")
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	output, err := exec.Command(luaRuntime(t), "queue.lua", "../../addon", path).CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	var payload struct{ Loaded, Receipt, Body, Code string }
	if err := json.Unmarshal(output, &payload); err != nil {
		t.Fatal(err)
	}
	// The receipt omits the actor identity to save QR modules; the retained
	// session baseline supplies it before matching.
	baseline := bridge.SignalIdentity{
		Release: d.Release, Character: d.Character, Realm: d.Realm,
		Product: d.Product, Build: d.Build, SessionNonce: d.SessionNonce,
	}
	loaded := parseSessionSignal(t, []byte(payload.Loaded), baseline)
	expected := bridge.SignalExpectation{Release: d.Release, Kind: "loaded", SessionNonce: d.SessionNonce, ReloadNonce: d.ReloadNonce, RequestID: d.RequestID, Character: d.Character, Realm: d.Realm, Product: d.Product, Build: d.Build}
	if err := loaded.Match(expected); err != nil {
		t.Fatal(err)
	}
	expected.Kind, expected.ReloadNonce, expected.AfterSequence = "reported", "", loaded.Sequence
	report, err := bridge.VerifyReport([]byte(payload.Receipt), []byte(payload.Body), []byte(code), expected)
	if err != nil {
		t.Fatal(err)
	}
	if payload.Code != code || !strings.Contains(string(report.Body), "世界") {
		t.Fatal("code or result bytes changed")
	}
	receiptPath := filepath.Join(t.TempDir(), "receipt.json")
	if err := os.WriteFile(receiptPath, []byte(payload.Receipt), 0600); err != nil {
		t.Fatal(err)
	}
	output, err = exec.Command(luaRuntime(t), "queue_ack.lua", "../../addon", path, receiptPath, strconv.FormatUint(report.Receipt.Sequence, 10)).CombinedOutput()
	if err != nil {
		t.Fatalf("ack queue: %v\n%s", err, output)
	}
	var cleanup struct{ Acknowledged, Ready, Saved string }
	if err := json.Unmarshal(output, &cleanup); err != nil {
		t.Fatal(err)
	}
	// Receipts omit the actor identity; the retained session baseline supplies
	// it, including the GUID the readiness comparison needs.
	ackBaseline := bridge.SignalIdentity{Release: d.Release, Character: d.Character, Realm: d.Realm, GUID: d.GUID, Product: d.Product, Build: d.Build, SessionNonce: d.SessionNonce}
	ack := parseSessionSignal(t, []byte(cleanup.Acknowledged), ackBaseline)
	ready := parseSessionSignal(t, []byte(cleanup.Ready), ackBaseline)
	if ready.Kind != "ready" || ready.Sequence <= ack.Sequence || !ready.InputReady || ready.RuntimeEpoch != 2 || ready.RequestID != "" || ready.GUID != d.GUID {
		t.Fatalf("invalid post-ACK readiness: %+v", ready)
	}
	expected.Kind, expected.AfterSequence = "acknowledged", report.Receipt.Sequence
	if err := ack.Match(expected); err != nil {
		t.Fatal(err)
	}
	if ack.ReportBytes != report.Receipt.ReportBytes || ack.ReportAdler32 != report.Receipt.ReportAdler32 || ack.CodeBytes != report.Receipt.CodeBytes || ack.CodeAdler32 != report.Receipt.CodeAdler32 {
		t.Fatal("ack payload identity changed")
	}
	before := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, d.RequestID, payload.Receipt, payload.Body)
	expected.Kind, expected.AfterSequence = "reported", loaded.Sequence
	if _, err := bridge.VerifyPersistedReportRemoval(strings.NewReader(before), strings.NewReader(cleanup.Saved), []byte(code), expected); err != nil {
		t.Fatalf("actual Lua removal: %v", err)
	}
}
