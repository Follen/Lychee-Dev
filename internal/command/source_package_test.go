package command

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

// This opt-in acceptance uses a sealed local release and its installed native
// executable. CI cannot manufacture the Windows LuaLS payload or fetch it.
func TestPackagedSourceSemanticCLIWhenAvailable(t *testing.T) {
	releaseRoot := os.Getenv("LYCHEEDEV_SOURCE_PACKAGE_RELEASE_ROOT")
	binary := os.Getenv("LYCHEEDEV_SOURCE_PACKAGE_BINARY")
	if releaseRoot == "" || binary == "" {
		t.Skip("set the isolated release root and installed native CLI paths")
	}
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := codebase.IndexFixtureSource(ctx, workspace, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	addon := filepath.Join(t.TempDir(), "Demo")
	if err := os.MkdirAll(addon, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(addon, "Demo.toc"), []byte("## Interface: 120100\n## Title: Demo\nMain.lua\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(addon, "Main.lua"), []byte("local info = C_AuctionHouse.GetRestrictedInfo('player')\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	response, code := invokeExecutable(t, binary, "source", "validate", "--snapshot", indexed.Snapshot.ID, "--path", addon, "--toc", "Demo.toc", "--semantic", "--release", releaseRoot, "--home", workspace, "--format=json")
	if code != 0 || !response.OK || len(response.Captures) != 1 {
		t.Fatalf("installed semantic CLI: %+v (%d)", response, code)
	}
	result := resultMap(t, response)
	semantic, ok := result["semantic"].(map[string]any)
	if !ok || semantic["runtime"] == nil || semantic["definitions"] == nil {
		t.Fatalf("installed CLI omitted semantic evidence: %#v", result)
	}
	captures, ok := semantic["captures"].(map[string]any)
	if !ok || len(captures) != 3 {
		t.Fatalf("LuaLS dependent capture IDs missing: %#v", semantic)
	}
	inputs, ok := semantic["inputs"].([]any)
	if !ok || len(inputs) != 1 {
		t.Fatalf("checked closure input missing: %#v", semantic)
	}
	if input, ok := inputs[0].(map[string]any); !ok || input["capture"] == nil {
		t.Fatalf("input capture ID missing: %#v", inputs)
	}
	mainCapture, ok := response.Captures[0].(map[string]any)
	if !ok {
		t.Fatalf("main capture malformed: %#v", response.Captures[0])
	}
	id, _ := mainCapture["id"].(string)
	_, err = vault.ReadWorkspace(ctx, workspace, func(s *vault.Store, m *vault.Metadata) (struct{}, error) {
		return struct{}{}, evidence.OpenArchive(s, m).VerifyCapture(ctx, id)
	})
	if err != nil {
		t.Fatalf("main semantic capture failed verification: %v", err)
	}
	query, queryCode := invokeExecutable(t, binary, "source", "query", "GetRestrictedInfo", "--snapshot", indexed.Snapshot.ID, "--topic", "api", "--home", workspace, "--format=json")
	if queryCode != 0 || !query.OK {
		t.Fatalf("installed query: %+v (%d)", query, queryCode)
	}
	results, ok := resultMap(t, query)["results"].([]any)
	if !ok {
		t.Fatalf("installed query results malformed: %+v", query)
	}
	var symbolID string
	for _, raw := range results {
		if item, ok := raw.(map[string]any); ok {
			symbolID, _ = item["symbolId"].(string)
			if symbolID != "" {
				break
			}
		}
	}
	if symbolID == "" {
		t.Fatalf("installed query had no symbolId: %+v", query)
	}
	for _, route := range []string{"refs", "context"} {
		research, researchCode := invokeExecutable(t, binary, "source", route, "--snapshot", indexed.Snapshot.ID, "--symbol-id", symbolID, "--home", workspace, "--format=json")
		if researchCode != 0 || !research.OK {
			t.Fatalf("installed %s: %+v (%d)", route, research, researchCode)
		}
		result := resultMap(t, research)
		if result["semanticRuntime"] == nil {
			t.Fatalf("installed %s did not discover bundled LuaLS: %#v", route, result)
		}
	}
}
