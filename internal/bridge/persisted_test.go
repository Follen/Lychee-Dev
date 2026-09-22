package bridge

import (
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"hash/adler32"
	"strings"
	"testing"
)

func TestPersistedReportRejectsInvalidState(t *testing.T) {
	for _, source := range []string{
		`LycheeDevDB={}`,
		`LycheeToolkitDB={schema=2,reports={}}`,
		`LycheeToolkitDB={schema={},reports={}}`,
		`LycheeToolkitDB={schema=1,reports="bad"}`,
		`LycheeToolkitDB={schema=1,reports={}}`,
		`LycheeToolkitDB={schema=1,reports={["OP-test"]={receipt=false,body="{}"}}}`,
		`LycheeToolkitDB={schema=1,reports={["OP-test"]={receipt="{}",body="{}"}}}`,
	} {
		if result, err := ReadPersistedReport(strings.NewReader(source), nil, SignalExpectation{RequestID: "OP-test"}); err == nil || len(result.Body) != 0 {
			t.Fatalf("accepted or partial: %s %+v %v", source, result, err)
		}
	}
}

func TestPersistedReportRemoval(t *testing.T) {
	code, body := []byte("return 42"), "42"
	signal := Signal{Schema: "lycheedev.signal.v1", Release: buildinfo.Version, Kind: "reported", SessionNonce: "session", RequestID: "OP-test", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.69875", Sequence: 3, CodeBytes: uint32(len(code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(code)), ReportBytes: 2, ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	raw, _ := json.Marshal(signal)
	before := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={["OP-test"]={receipt=%q,body=%q}}}`, raw, body)
	expected := SignalExpectation{Release: signal.Release, Kind: "reported", RequestID: "OP-test", SessionNonce: "session", Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build, AfterSequence: 2}
	for _, after := range []string{
		`LycheeToolkitDB={schema=1,reports={}}`,
		`LycheeToolkitDB={schema=1,reports={["OP-foreign"]={receipt="untouched",body="data"}},options={bridgeEnabled=true}}`,
	} {
		report, err := VerifyPersistedReportRemoval(strings.NewReader(before), strings.NewReader(after), code, expected)
		if err != nil || string(report.Body) != body || report.Receipt != signal {
			t.Fatalf("removal: %+v %v", report, err)
		}
	}
	for _, after := range []string{
		"", `LycheeToolkitDB={}`, `LycheeToolkitDB={schema=2,reports={}}`,
		`LycheeToolkitDB={schema=1,reports=false}`, `LycheeDevDB={schema=1,reports={}}`,
		`LycheeToolkitDB={schema=1,reports={}}; error("injected")`,
		`LycheeToolkitDB={schema=1,reports={["OP-test"]=false}}`,
		`LycheeToolkitDB={schema=1,reports={["OP-test"]={}}}`, before,
	} {
		if report, err := VerifyPersistedReportRemoval(strings.NewReader(before), strings.NewReader(after), code, expected); err == nil || len(report.Body) != 0 {
			t.Fatalf("invalid removal accepted: %s %v", after, err)
		}
	}
	for _, invalidBefore := range []string{"", `LycheeToolkitDB={schema=1,reports={}}`, strings.Replace(before, "Paladin", "Other", 1)} {
		strict := expected
		strict.Character = "Paladin"
		if _, err := VerifyPersistedReportRemoval(strings.NewReader(invalidBefore), strings.NewReader(`LycheeToolkitDB={schema=1,reports={}}`), code, strict); err == nil {
			t.Fatal("absence accepted without matching original report")
		}
	}
}
