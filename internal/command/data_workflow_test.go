package command

import (
	"fmt"
	"testing"
)

// Exercise the skill's name -> bounded candidates -> relation workflow through
// the real CLI. This verifies command composition, not an agent's reasoning.
func TestDataNameDiscoveryWorkflow(t *testing.T) {
	root, pin := newDataFixture(t, []fixtureTable{
		{name: "SpellName", fileDataID: 102, columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Name_lang", kind: 's', loc: true},
		}, rows: [][]any{{10, fixtureStr("Claw")}, {20, fixtureStr("Claw")}, {30, fixtureStr("Claw Slam")}}},
		{name: "SpellEffect", fileDataID: 104, columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "SpellID", kind: 'i', bits: 32, foreign: "SpellName::ID"},
			{name: "EffectBasePointsF", kind: 'f'},
		}, rows: [][]any{{1, 10, float32(4)}, {2, 20, float32(8)}, {3, 30, float32(99)}}},
	})
	query := func(args ...string) map[string]any {
		t.Helper()
		got, code := invoke(t, append(args, dataArgs(pin, root, "--format=json")...)...)
		if code != 0 || !got.OK || got.Context["snapshot"] != pin || len(got.Captures) == 0 {
			t.Fatalf("%v: exit=%d envelope=%+v", args, code, got)
		}
		return got.Result.(map[string]any)
	}
	schema := query("data", "db2", "schema", "SpellName")
	field := ""
	for _, raw := range schema["fields"].([]any) {
		f := raw.(map[string]any)
		if f["name"] == "Name_lang" {
			field = f["name"].(string)
		}
	}
	if field == "" {
		t.Fatal("localized name missing from schema")
	}
	search := query("data", "db2", "search", "SpellName", "--field", field, "--query", "Claw", "--limit", "1")
	if search["truncated"] != true || search["count"] != float64(1) {
		t.Fatalf("bounded candidate search lost truncation: %+v", search)
	}
	// Search has no cursor. Continue the exact-name scope with ordered SQL;
	// duplicates must survive and an unrelated substring match must not leak in.
	var ids []int
	after := 0
	for page := 0; page < 3; page++ {
		result := query("data", "sql", "--sql", "SELECT ID FROM SpellName WHERE Name_lang=:name AND ID>:after ORDER BY ID LIMIT 1", "--param", `name="Claw"`, "--param", fmt.Sprintf("after=%d", after))
		rows := result["result"].(map[string]any)["rows"].([]any)
		if len(rows) == 0 {
			break
		}
		after = int(rows[0].([]any)[0].(float64))
		ids = append(ids, after)
	}
	if fmt.Sprint(ids) != "[10 20]" {
		t.Fatalf("candidate continuation: %v", ids)
	}
	effectSchema := query("data", "db2", "schema", "SpellEffect")
	if effectSchema["table"] != "SpellEffect" {
		t.Fatal(effectSchema)
	}
	for i, id := range ids {
		effects := query("data", "db2", "foreign-key", "SpellEffect", "--field", "SpellID", "--value", fmt.Sprint(id), "--limit", "10")
		rows := effects["rows"].([]any)
		if len(rows) != 1 || rows[0].(map[string]any)["SpellID"] != float64(id) || rows[0].(map[string]any)["EffectBasePointsF"] != float64(4*(i+1)) {
			t.Fatalf("crossed same-name candidates: %+v", effects)
		}
	}
	// A complete empty lookup is separate from encrypted/partial empty coverage,
	// which is exercised by TestSQLIdentityLookupPreservesPartialCoverage.
	empty := query("data", "db2", "search", "SpellName", "--field", field, "--query", "Absent", "--limit", "10")
	if empty["count"] != float64(0) || empty["partial"] == true || empty["truncated"] == true {
		t.Fatalf("complete empty scope: %+v", empty)
	}
}
