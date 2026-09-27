package luals

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
)

// The pinned LuaLS executable must resolve a real generated Blizzard API
// declaration back to the stable synthetic file and the original source span.
func TestRealRuntimeGeneratedEnvironment(t *testing.T) {
	runtimeDir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	docsDir := os.Getenv("LYCHEEDEV_ENVIRONMENT_TEST_DOCS")
	if runtimeDir == "" || docsDir == "" {
		t.Skip("set LYCHEEDEV_LUALS_TEST_RUNTIME and LYCHEEDEV_ENVIRONMENT_TEST_DOCS")
	}
	docs := map[string][]byte{}
	for _, name := range []string{"retail-UnitDocumentation.lua", "retail-ContainerDocumentation.lua", "retail-AuctionHouseEnumsDocumentation.lua"} {
		data, err := os.ReadFile(filepath.Join(docsDir, name))
		if err != nil {
			t.Fatal(err)
		}
		docs[strings.TrimPrefix(name, "retail-")] = data
	}
	generated, err := environment.Build(context.Background(), environment.Identity{Repository: "Gethe/wow-ui-source", Commit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b", Client: "retail", GeneratorVersion: environment.GeneratorVersion}, docs, environment.Limits{})
	if err != nil {
		t.Fatal(err)
	}
	source := "local health = UnitHealth(\"player\")\nlocal bag = C_Container.ContainerIDToInventoryID(0)\nlocal value = Enum.AuctionHouseError.NotEnoughMoney\n"
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "probe.lua"), []byte(source), 0600); err != nil {
		t.Fatal(err)
	}
	r := &Runtime{directory: runtimeDir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 70*time.Second)
	defer cancel()
	var health *environment.DefinitionMapping
	for i := range generated.Mappings {
		if generated.Mappings[i].Callable == "UnitHealth" {
			health = &generated.Mappings[i]
			break
		}
	}
	if health == nil {
		t.Fatal("UnitHealth mapping missing")
	}
	// Library preloadFileSize is 1 MiB so oversized source datasets stay
	// excluded. The generated API library must still resolve above that cap.
	definitions := append(append([]byte{}, generated.Definitions...), []byte("--"+strings.Repeat(" ", 1<<20)+"\n")...)
	analysis, err := r.AnalyzeWorkspace(ctx, workspace, definitions, []Query{
		{Kind: Definition, Path: "probe.lua", Position: Position{Line: 0, Character: strings.Index(strings.Split(source, "\n")[0], "UnitHealth")}},
		{Kind: Definition, Path: "probe.lua", Position: Position{Line: 1, Character: strings.Index(strings.Split(source, "\n")[1], "ContainerIDToInventoryID")}},
		{Kind: Definition, Path: "probe.lua", Position: Position{Line: 2, Character: strings.Index(strings.Split(source, "\n")[2], "NotEnoughMoney")}},
		{Kind: References, Path: DefinitionPath, Position: Position{Line: health.GeneratedLine, Character: health.GeneratedUTF16Column}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if analysis.State != "complete" || len(analysis.Results) != 4 {
		t.Fatalf("analysis: %+v", analysis)
	}
	for i, query := range analysis.Results[:3] {
		if len(query.Locations) == 0 || query.Locations[0].Path != DefinitionPath {
			t.Fatalf("query %d unresolved: %+v", i, query)
		}
	}
	refFound := false
	for _, loc := range analysis.Results[3].Locations {
		if loc.Path == "probe.lua" {
			refFound = true
		}
	}
	if !refFound {
		t.Fatalf("generated declaration references omitted probe.lua: %+v", analysis.Results[3])
	}
	if loc := analysis.Results[0].Locations[0]; loc.Range.Start.Line != health.GeneratedLine || loc.Range.Start.Character != health.GeneratedUTF16Column {
		t.Fatalf("generated mapping mismatch: got %+v, expected %+v", loc, *health)
	}
	checked, err := r.Check(ctx, map[string][]byte{"probe.lua": []byte(source)}, definitions)
	if err != nil {
		t.Fatal(err)
	}
	for _, diagnostic := range checked.Diagnostics {
		if strings.Contains(strings.ToLower(diagnostic.Message), "syntax") {
			t.Fatalf("LuaLS syntax diagnostic: %+v", diagnostic)
		}
	}
}

// An external API declaration cannot be called complete merely because its
// own fallback scope answered. This optional pinned full-tree test checks that
// the worktree has finished loading before incoming references are queried.
func TestRealRuntimeFullRetailAPIReferences(t *testing.T) {
	runtimeDir := os.Getenv("LYCHEEDEV_LUALS_TEST_RUNTIME")
	workspace := os.Getenv("LYCHEEDEV_LUALS_REAL_RETAIL_WORKSPACE")
	definitionsPath := os.Getenv("LYCHEEDEV_LUALS_REAL_RETAIL_DEFINITIONS")
	if runtimeDir == "" || workspace == "" || definitionsPath == "" {
		t.Skip("set LuaLS runtime, full Retail workspace and generated definitions")
	}
	definitions, err := os.ReadFile(definitionsPath)
	if err != nil {
		t.Fatal(err)
	}
	marker := []byte("\nfunction UnitHealth(")
	offset := bytes.Index(definitions, marker)
	if offset < 0 {
		t.Fatal("UnitHealth declaration missing")
	}
	position := Position{Line: bytes.Count(definitions[:offset+1], []byte("\n")), Character: len("function ")}
	r := &Runtime{directory: runtimeDir, Identity: Pinned()}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	analysis, err := r.AnalyzeWorkspace(ctx, workspace, definitions, []Query{{Kind: References, Path: DefinitionPath, Position: position}})
	if err != nil {
		t.Fatal(err)
	}
	if len(analysis.Results) != 1 || analysis.Results[0].State != "complete" {
		t.Fatalf("incomplete API references: %+v", analysis)
	}
	for _, loc := range analysis.Results[0].Locations {
		if loc.Path != DefinitionPath {
			return
		}
	}
	t.Fatalf("UnitHealth callsites absent after full workspace load: %+v", analysis.Results[0])
}
