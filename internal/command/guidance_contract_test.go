package command

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSchemaRequiredFlagsMatchOmissionErrors(t *testing.T) {
	for _, tc := range []struct {
		route string
		flag  string
		args  []string
	}{
		{"source batch", "--file", []string{"--snapshot", "PIN-unused"}},
		{"asset search", "--listfile", []string{"--snapshot", "PIN-unused", "--query", "icon"}},
		{"asset inspect", "--file-id", []string{"--snapshot", "PIN-unused", "--cdn"}},
		{"asset export", "--file-id", []string{"--snapshot", "PIN-unused", "--cdn", "--output", "unused"}},
		{"asset export", "--output", []string{"--snapshot", "PIN-unused", "--cdn", "--file-id", "1"}},
		{"asset demux", "--output", []string{"--path", "unused.avi"}},
		{"evidence bundle", "--ids", []string{"--output", "unused.zip"}},
		{"evidence bundle", "--output", []string{"--ids", "CAP-unused"}},
		{"skill install", "--path", []string{"--release", "unused"}},
		{"addon install", "--installation", []string{"--release", "unused"}},
	} {
		t.Run(tc.route+"/"+tc.flag, func(t *testing.T) {
			words := strings.Fields(tc.route)
			response, code := invoke(t, append(append([]string{}, words...), append(tc.args, "--format=json")...)...)
			if code != 2 || response.Error == nil || !strings.Contains(response.Error.Message, tc.flag) {
				t.Fatalf("omission should fail before IO: code=%d response=%+v", code, response)
			}
			schema, code := invoke(t, append(append([]string{"schema"}, words...), "--format=json")...)
			if code != 0 {
				t.Fatalf("schema: %+v", schema)
			}
			var required bool
			for _, input := range resultMap(t, schema)["inputs"].([]any) {
				field := input.(map[string]any)
				if field["name"] == tc.flag {
					required = field["required"] == true
				}
			}
			if !required {
				t.Errorf("schema marks %s optional but execution requires it", tc.flag)
			}
		})
	}
}

func TestLiveBugsCountSchemaAndParserBound(t *testing.T) {
	schema, code := invoke(t, "schema", "live", "bugs", "--format=json")
	if code != 0 {
		t.Fatal(schema)
	}
	for _, input := range resultMap(t, schema)["inputs"].([]any) {
		field := input.(map[string]any)
		if field["name"] == "--count" && field["maximum"] != float64(100) {
			t.Errorf("live bugs advertises count %v, execution accepts at most 100", field["maximum"])
		}
	}
	if _, err := parseOptions([]string{"live", "bugs", "--count", "100"}); err != nil {
		t.Fatal(err)
	}
	if _, err := parseOptions([]string{"live", "bugs", "--count", "101"}); err == nil {
		t.Error("live bugs accepts count above its execution limit")
	}
	if _, err := parseOptions([]string{"source", "inspect", "--count", "2000"}); err != nil {
		t.Fatalf("source inspection must keep its separate limit: %v", err)
	}
}

func TestSourceBatchInputSchemaKeepsDefaultedFieldsOptional(t *testing.T) {
	file := filepath.Join(t.TempDir(), "batch.json")
	if err := os.WriteFile(file, []byte(`{"requests":[{"query":{"text":"GetCVar"}},{"refs":{"symbol":"GetCVar"}},{"context":{"symbol":"GetCVar"}}]}`), 0600); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := indexGitFixture(t, context.Background(), root, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "source", "batch", "--file", file, "--snapshot", indexed.Snapshot.ID, "--static-only", "--home", root, "--format=json")
	if code != 0 || !result.OK {
		t.Fatalf("minimal input failed to execute: %d %+v", code, result)
	}
	response, code := invoke(t, "schema", "source", "batch", "--format=json")
	if code != 0 {
		t.Fatal(response)
	}
	input := resultMap(t, response)["details"].(map[string]any)["jsonInput"].(map[string]any)
	item := input["properties"].(map[string]any)["requests"].(map[string]any)["items"].(map[string]any)
	properties := item["properties"].(map[string]any)
	for _, kind := range []string{"query", "refs", "context"} {
		shape := properties[kind].(map[string]any)["anyOf"].([]any)[0].(map[string]any)
		for _, field := range shape["required"].([]any) {
			if kind != "query" || field != "text" {
				t.Errorf("%s.%s is accepted by omission but schema requires it", kind, field)
			}
		}
	}
}

func TestSQLInputSchemaRejectsNonScalarParameters(t *testing.T) {
	if _, err := decodeDataQuery([]byte(`{"sql":"SELECT $id","parameters":{"id":{"nested":1}}}`)); err == nil {
		t.Fatal("nested parameters unexpectedly accepted")
	}
	response, code := invoke(t, "schema", "data", "sql", "--format=json")
	if code != 0 {
		t.Fatal(response)
	}
	input := resultMap(t, response)["details"].(map[string]any)["jsonInput"].(map[string]any)
	parameters := input["properties"].(map[string]any)["parameters"].(map[string]any)
	scalar := parameters["additionalProperties"].(map[string]any)
	raw, _ := json.Marshal(scalar["type"])
	if string(raw) != `["string","number","boolean","null"]` || parameters["type"] != "object" {
		t.Errorf("parameter schema must describe the accepted scalar object: %+v", parameters)
	}
}

func TestSnapshotSchemaRespectsOperationsWithoutPins(t *testing.T) {
	t.Chdir(t.TempDir())
	response, code := invoke(t, "source", "list", "--format=json")
	if code != 0 || !response.OK {
		t.Fatalf("repository list must work without a project or snapshot: %d %+v", code, response)
	}
	for _, tc := range []struct{ route, exception string }{
		{"source list", ""},
		{"asset demux", "--path"},
		{"source validate", "--matrix"},
	} {
		schema, code := invoke(t, append(append([]string{"schema"}, strings.Fields(tc.route)...), "--format=json")...)
		if code != 0 {
			t.Fatal(schema)
		}
		for _, input := range resultMap(t, schema)["inputs"].([]any) {
			field := input.(map[string]any)
			if field["name"] != "--snapshot" {
				continue
			}
			condition, _ := field["requiredWhen"].(string)
			if tc.exception == "" && condition != "" || tc.exception != "" && !strings.Contains(condition, tc.exception) {
				t.Errorf("%s snapshot condition omits the no-pin mode: %q", tc.route, condition)
			}
		}
	}
}
