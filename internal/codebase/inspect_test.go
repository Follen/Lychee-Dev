package codebase

import (
	"context"
	"strings"
	"testing"
)

func TestInspectPathReturnsFileAndAssetRows(t *testing.T) {
	files := topicTree()
	files["Interface/AddOns/Test/Core.lua"] = "line one\nline two\nline three\nline four\nline five\nline six\nline seven\n"
	b, pin := indexedSearchFixture(t, files)
	ctx := context.Background()

	response, err := b.InspectTarget(ctx, "PIN-TEST", pin, TargetQuery{Path: "Core.lua"})
	if err != nil {
		t.Fatal(err)
	}
	if !response.Complete || response.Truncated || len(response.Results) != 1 {
		t.Fatalf("file rows = %+v", response)
	}
	file := response.Results[0]
	if file.Kind != "file" || file.Name != "Core.lua" || file.MatchedBy != "path" || file.Score != 100 || file.ScoreParts["match"] != 100 {
		t.Fatalf("file row = %+v", file)
	}
	if !strings.HasPrefix(file.Excerpt, "1: line one") || !strings.Contains(file.Excerpt, "6: line six") || file.ContentHash == "" {
		t.Fatalf("file excerpt/hash = %+v", file)
	}

	response, err = b.InspectTarget(ctx, "PIN-TEST", pin, TargetQuery{Path: "Textures"})
	if err != nil {
		t.Fatal(err)
	}
	if len(response.Results) != 1 {
		t.Fatalf("asset rows = %+v", response.Results)
	}
	asset := response.Results[0]
	if asset.Kind != "asset" || asset.Role != "project" || asset.MatchedBy != "path" {
		t.Fatalf("asset row = %+v", asset)
	}
	if !strings.Contains(asset.Excerpt, "format=png") || !strings.Contains(asset.Excerpt, "mime=image/png") ||
		!strings.Contains(asset.Excerpt, "width=64") || !strings.Contains(asset.Excerpt, "height=32") ||
		!strings.Contains(asset.Excerpt, "local=") || !strings.Contains(asset.Excerpt, "blobs") {
		t.Fatalf("asset metadata excerpt = %q", asset.Excerpt)
	}
}

func TestInspectSymbolExactPathInspectsPath(t *testing.T) {
	b, pin := indexedSearchFixture(t, topicTree())
	response, err := b.InspectTarget(context.Background(), "PIN-TEST", pin, TargetQuery{Symbol: "Interface/AddOns/Test/Core.lua"})
	if err != nil {
		t.Fatal(err)
	}
	if len(response.Results) == 0 || response.Results[0].Kind != "file" {
		t.Fatalf("exact-path symbol did not inspect the path: %+v", response.Results)
	}
}

func TestInspectSymbolFallsBackToBoundedSearch(t *testing.T) {
	files := map[string]string{}
	source := strings.Builder{}
	for i := 0; i < 30; i++ {
		source.WriteString("function Gadget" + string(rune('A'+i/10)) + string(rune('0'+i%10)) + "()\nend\n")
	}
	files["Gadgets.lua"] = source.String()
	b, pin := indexedSearchFixture(t, files)
	response, err := b.InspectTarget(context.Background(), "PIN-TEST", pin, TargetQuery{Symbol: "Gadget"})
	if err != nil {
		t.Fatal(err)
	}
	if len(response.Results) != inspectTargetLimit || !response.Truncated || response.Complete {
		t.Fatalf("fallback search bound: %d results truncated=%v complete=%v", len(response.Results), response.Truncated, response.Complete)
	}
	for _, match := range response.Results {
		if match.MatchedBy != "symbol_prefix" {
			t.Fatalf("fallback rows should come from search: %+v", match)
		}
	}
}

func TestInspectTargetAdmission(t *testing.T) {
	b, pin := indexedSearchFixture(t, topicTree())
	ctx := context.Background()
	if _, err := b.InspectTarget(ctx, "PIN-TEST", pin, TargetQuery{}); err == nil || !strings.Contains(err.Error(), "codebase.inspect_target_required") {
		t.Fatalf("empty target accepted: %v", err)
	}
	if _, err := b.InspectTarget(ctx, "PIN-TEST", pin, TargetQuery{Symbol: "x", Path: "y"}); err == nil || !strings.Contains(err.Error(), "codebase.inspect_target_conflict") {
		t.Fatalf("conflicting targets accepted: %v", err)
	}
}
