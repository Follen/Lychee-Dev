package bridge

import (
	"strings"
	"testing"
)

func TestSavedStateTypedKeysAndEscapes(t *testing.T) {
	text := `LycheeToolkitDB = { [1]="number", ["1"]="string", title="\228\184\173\230\150\135", absent=nil, nested={false,true}, }`
	globals, err := DecodeSavedState(strings.NewReader(text), SavedStateLimits())
	if err != nil {
		t.Fatal(err)
	}
	table := globals["LycheeToolkitDB"].(LiteralTable)
	if table[float64(1)] != "number" || table["1"] != "string" || table["title"] != "中文" {
		t.Fatalf("%v", table)
	}
	if _, ok := table["absent"]; ok {
		t.Fatal("nil field retained")
	}
}
func TestSavedStateRejectsCodeAndCorruption(t *testing.T) {
	for _, text := range []string{`x=os.execute("bad")`, `x=function() end`, `x={}; x={}`, `x={[nil]=1}`, `x={[{}]=1}`, `x={a=1,a=2}`, `x="\999"`, `x="\255"`, `x="\zbad"`, `x=1+2`, `x={1 2}`, `x=1e999`, `x="unterminated`} {
		if _, err := DecodeSavedState(strings.NewReader(text), SavedStateLimits()); err == nil {
			t.Errorf("accepted %q", text)
		}
	}
}
func TestSavedStateBudgets(t *testing.T) {
	for _, limit := range []DecodeLimits{{FileBytes: 3, StringBytes: 100, Entries: 100, Depth: 10}, {FileBytes: 100, StringBytes: 1, Entries: 100, Depth: 10}, {FileBytes: 100, StringBytes: 100, Entries: 1, Depth: 10}, {FileBytes: 100, StringBytes: 100, Entries: 100, Depth: 1}} {
		if _, err := DecodeSavedState(strings.NewReader(`x={a={b="hello"}}`), limit); err == nil {
			t.Fatalf("budget not enforced: %+v", limit)
		}
	}
}

func TestToolkitRootDoesNotImportLegacyState(t *testing.T) {
	for _, input := range []string{`LycheeDevDB={}`, `DumperDB={}`, `LycheeToolkitDB={} LycheeDevDB={}`, `LycheeToolkitDB=nil`} {
		if _, err := ReadToolkitState(strings.NewReader(input), SavedStateLimits()); err == nil {
			t.Fatalf("accepted old or ambiguous state: %s", input)
		}
	}
	if _, err := ReadToolkitState(strings.NewReader(`LycheeToolkitDB={}`), SavedStateLimits()); err != nil {
		t.Fatal(err)
	}
}
