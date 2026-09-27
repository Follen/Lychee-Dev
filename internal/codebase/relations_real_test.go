package codebase

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/luals"
)

func TestRealLuaLSMapsAPIReferencesAndDefinitions(t *testing.T) {
	root := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	if root == "" {
		t.Skip("set LYCHEEDEV_LUALS_TEST_RUNTIME for public LSP integration")
	}
	release := t.TempDir()
	destination := filepath.Join(release, "payload", "tool", "luals")
	err := filepath.WalkDir(root, func(name string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		rel, err := filepath.Rel(root, name)
		if err != nil {
			return err
		}
		if rel == "." {
			return os.MkdirAll(destination, 0o755)
		}
		target := filepath.Join(destination, rel)
		if entry.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		if !entry.Type().IsRegular() {
			return nil
		}
		raw, err := os.ReadFile(name)
		if err != nil {
			return err
		}
		return os.WriteFile(target, raw, 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	identity, err := json.Marshal(luals.Pinned())
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(destination, "runtime.json"), identity, 0o644); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	runtime, err := luals.Open(ctx, release, func(context.Context, string) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	b, seed := sourceFixture(t)
	apiPath := "Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua"
	api := `local Unit = { Name="Unit", Type="System", Functions={
 {Name="UnitHealth",Type="Function",SecretReturns=true,Returns={{Name="health",Type="number"}}},
}}; APIDocumentation:AddDocumentationTable(Unit)`
	pin := testCommit(t, b, seed, map[string]string{apiPath: api, "probe.lua": "function Foo()\n local health = UnitHealth('player')\n return health\nend\n"}, "API semantic mapping")
	if _, err := b.IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	apiRefs, err := b.Relate(ctx, "fixed", pin, RelationQuery{Symbol: "UnitHealth", Direction: "incoming", Limit: 25}, ResearchOptions{Semantic: runtime})
	if err != nil {
		t.Fatal(err)
	}
	if apiRefs.EnvironmentManifest == nil || apiRefs.EnvironmentManifest.Identity.Commit != pin.ExactCommit {
		t.Fatalf("API environment identity = %+v", apiRefs)
	}
	resolvedIncoming := false
	for _, relation := range apiRefs.Relations {
		if relation.State == "resolved" && relation.Path == "probe.lua" && relation.Kind == "reference" {
			resolvedIncoming = true
		}
		if relation.Path == luals.DefinitionPath {
			t.Fatalf("synthetic definition leaked as source relation: %+v", relation)
		}
	}
	if !resolvedIncoming {
		t.Fatalf("API incoming refs not resolved: %+v", apiRefs)
	}
	outgoing, err := b.Relate(ctx, "fixed", pin, RelationQuery{Symbol: "Foo", Direction: "outgoing", Limit: 25}, ResearchOptions{Semantic: runtime})
	if err != nil {
		t.Fatal(err)
	}
	resolvedOutgoing := false
	for _, relation := range outgoing.Relations {
		if relation.State == "resolved" && relation.Target == "UnitHealth" && relation.TargetPath == apiPath && relation.TargetCommit == pin.ExactCommit && strings.HasPrefix(relation.TargetID, "API-") {
			resolvedOutgoing = true
		}
	}
	if !resolvedOutgoing {
		t.Fatalf("API outgoing definition not mapped to original pinned source: %+v", outgoing)
	}
}
