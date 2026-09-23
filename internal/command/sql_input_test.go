package command

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestSQLInputSelectionAndParameters(t *testing.T) {
	file := filepath.Join(t.TempDir(), "query.sql")
	if err := os.WriteFile(file, []byte("SELECT ID FROM SpellName WHERE ID=:id"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, args := range [][]string{
		{"data", "sql", "--file", file, "--sql", "SELECT 1"},
		{"data", "sql", "--sql", "SELECT 1", "--stdin"},
		{"data", "sql", "--param", "id=1"},
		{"data", "sql", "--sql", "SELECT 1", "--param", "id=1", "--param", "id=2"},
	} {
		result, code := invoke(t, append(args, "--snapshot", "PIN-test", "--cdn", "--format=json")...)
		if code != 2 || result.OK {
			t.Fatalf("accepted invalid SQL source or binding %v: %+v (%d)", args, result, code)
		}
	}
	opts, err := parseOptions([]string{"data", "sql", "--sql", "SELECT :id, :label, :enabled, :nil", "--param", "id=9007199254740993", "--param", "label=001", "--param", "enabled=true", "--param", "nil=null"})
	if err != nil {
		t.Fatal(err)
	}
	query, err := readSelectedDataQuery(opts)
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]any{"id": parseQueryScalar("9007199254740993"), "label": "001", "enabled": true, "nil": nil}
	if !reflect.DeepEqual(query.Parameters, want) {
		t.Fatalf("parameters=%#v want=%#v", query.Parameters, want)
	}
	opts, err = parseOptions([]string{"data", "sql", "--file", file})
	if err != nil {
		t.Fatal(err)
	}
	query, err = readSelectedDataQuery(opts)
	if err != nil || query.SQL != "SELECT ID FROM SpellName WHERE ID=:id" {
		t.Fatalf("plain SQL file: %+v %v", query, err)
	}
}

func TestSQLCSVExportFromPinnedFixture(t *testing.T) {
	workspace, pin := newDataFixture(t, []fixtureTable{{name: "TestNames", fileDataID: 91,
		columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Name", kind: 's'}},
		rows:    [][]any{{1, "Hello, world"}, {2, "Second"}},
	}})
	output := filepath.Join(t.TempDir(), "query.csv")
	args := []string{"data", "sql", "--sql", "SELECT ID, Name FROM TestNames WHERE ID=:id", "--param", "id=1", "--snapshot", pin, "--cdn", "--offline", "--encoding", "csv", "--output", output, "--home", workspace, "--format=json"}
	result, code := invoke(t, args...)
	if code != 0 || !result.OK || len(result.Captures) != 2 || resultMap(t, result)["schema"] != "lycheedev.query-csv.v1" {
		t.Fatalf("CSV export: %+v (%d)", result, code)
	}
	raw, err := os.ReadFile(output)
	if err != nil || !strings.Contains(string(raw), "Hello, world") {
		t.Fatalf("CSV bytes: %q %v", raw, err)
	}
	if _, err := os.Stat(output + ".manifest.json"); err != nil {
		t.Fatal(err)
	}
	conflict, code := invoke(t, args...)
	if code != 3 || conflict.OK || conflict.Error.Code != "records.export_conflict" {
		t.Fatalf("CSV overwrite refusal: %+v (%d)", conflict, code)
	}
}
