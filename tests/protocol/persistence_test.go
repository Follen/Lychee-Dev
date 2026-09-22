package protocol_test

import (
	"bytes"
	"os/exec"
	"testing"

	"github.com/follenfang/lycheedev/internal/buildinfo"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestCommittedLuaStateReadByGo(t *testing.T) {
	lua := luaRuntime(t)
	output, err := exec.Command(lua, "persistence.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	expected := bridge.SignalExpectation{Kind: "reported", Release: buildinfo.Version, SessionNonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", RequestID: "OP-persisted", Character: "character", Realm: "realm", Product: "retail", Build: "12.1.0.69875", AfterSequence: 3}
	report, err := bridge.ReadPersistedReport(bytes.NewReader(output), []byte("return 42"), expected)
	if err != nil {
		t.Fatal(err)
	}
	if string(report.Body) != `{"answer":42,"text":"世界"}` {
		t.Fatalf("wrong original body: %s", report.Body)
	}
	if _, err := bridge.ReadPersistedReport(bytes.NewReader(output), []byte("return 99"), expected); err == nil {
		t.Fatal("changed code accepted")
	}
	expected.RequestID = "OP-missing"
	if _, err := bridge.ReadPersistedReport(bytes.NewReader(output), []byte("return 42"), expected); err == nil {
		t.Fatal("missing request accepted")
	}
}
