package command

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSourceBatchThroughCLI(t *testing.T) {
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := codebase.IndexFixtureSource(context.Background(), root, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(t.TempDir(), "requests.json")
	if err := os.WriteFile(file, []byte(`{"requests":[{"id":"find","query":{"text":"GetCVar","limit":1}},{"id":"refs","refs":{"symbol":"GetRestrictedInfo"}},{"id":"context","context":{"symbol":"GetRestrictedInfo","depth":1,"maxBytes":32}}]}`), 0600); err != nil {
		t.Fatal(err)
	}
	args := []string{"source", "batch", "--file", file, "--snapshot", indexed.Snapshot.ID, "--static-only", "--home", root, "--format=json"}
	response, code := invoke(t, args...)
	if code != 0 || !response.OK || len(response.Captures) != 1 || response.Context["snapshot"] != indexed.Snapshot.ID {
		t.Fatalf("batch: %+v (%d)", response, code)
	}
	result := resultMap(t, response)
	rows, ok := result["results"].([]any)
	if !ok || len(rows) != 3 || result["snapshot"] != indexed.Snapshot.ID || rows[0].(map[string]any)["id"] != "find" {
		t.Fatalf("batch identities missing: %#v", result)
	}
	if err := os.WriteFile(file, []byte(`{"requests":[{"query":{"text":"GetCVar"},"refs":{"symbol":"GetCVar"}}]}`), 0600); err != nil {
		t.Fatal(err)
	}
	bad, code := invoke(t, args...)
	if code != 2 || bad.OK || bad.Error.Code != "codebase.invalid_batch" {
		t.Fatalf("invalid batch: %+v (%d)", bad, code)
	}
	if err := os.WriteFile(file, []byte(`{"requests":[{"query":{"text":"GetCVar","topic":"missing"}}]}`), 0600); err != nil {
		t.Fatal(err)
	}
	bad, code = invoke(t, args...)
	if code != 2 || bad.OK || bad.Error.Code != "codebase.invalid_query" {
		t.Fatalf("invalid batch query: %+v (%d)", bad, code)
	}
}
