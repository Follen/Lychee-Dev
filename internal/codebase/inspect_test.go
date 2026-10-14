package codebase

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestInspectPathReturnsFileAndAssetRows(t *testing.T) {
	files := topicTree()
	files["Interface/AddOns/Test/Core.lua"] = "-- line one\n-- line two\n-- line three\n-- line four\n-- line five\n-- line six\n-- line seven\n"
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
	if !strings.HasPrefix(file.Excerpt, "1: -- line one") || !strings.Contains(file.Excerpt, "6: -- line six") || file.ContentHash == "" {
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

func TestInspectSourceTargetPreservesIndexCoverageAndCapture(t *testing.T) {
	for _, skipped := range []bool{false, true} {
		t.Run(map[bool]string{false: "complete_index", true: "skipped_file"}[skipped], func(t *testing.T) {
			ctx := context.Background()
			fixture := t.TempDir()
			if err := os.WriteFile(filepath.Join(fixture, "Good.lua"), []byte("return 1\n"), 0644); err != nil {
				t.Fatal(err)
			}
			if skipped {
				f, err := os.Create(filepath.Join(fixture, "Large.lua"))
				if err != nil {
					t.Fatal(err)
				}
				if err := f.Truncate(maxSourceBytes + 1); err != nil {
					f.Close()
					t.Fatal(err)
				}
				if err := f.Close(); err != nil {
					t.Fatal(err)
				}
			}
			store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
			if err != nil {
				t.Fatal(err)
			}
			indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", fixture)
			if err != nil {
				t.Fatal(err)
			}
			if indexed.Summary.Complete == skipped || skipped && indexed.Summary.SkippedDocuments != 1 {
				t.Fatalf("fixture coverage = %+v", indexed.Summary)
			}
			for _, target := range []string{"Large.lua", "Good.lua", "Missing.lua"} {
				reading, err := InspectSourceTarget(ctx, store.Root(), indexed.Snapshot.ID, TargetQuery{Path: target})
				if err != nil {
					t.Fatal(err)
				}
				wantRows := 0
				if target == "Good.lua" {
					wantRows = 1
				}
				if len(reading.Result.Results) != wantRows || reading.Result.Complete == skipped || reading.Result.Truncated || reading.Capture.Complete != reading.Result.Complete {
					t.Fatalf("%s inspect = %+v", target, reading)
				}
				if !reflect.DeepEqual(reading.Result.Coverage, indexed.Summary.Coverage()) {
					t.Fatalf("%s coverage = %+v, want %+v", target, reading.Result.Coverage, indexed.Summary.Coverage())
				}
				stored, err := vault.ReadWorkspace(ctx, store.Root(), func(s *vault.Store, m *vault.Metadata) (SearchResponse, error) {
					_, raw, err := evidence.OpenArchive(s, m).FetchCapture(ctx, reading.Capture.ID, 1<<20)
					if err != nil {
						return SearchResponse{}, err
					}
					var result SearchResponse
					err = json.Unmarshal(raw, &result)
					return result, err
				})
				if err != nil || !reflect.DeepEqual(stored.Coverage, reading.Result.Coverage) || stored.Complete != reading.Result.Complete {
					t.Fatalf("%s archived coverage = %+v, %v", target, stored, err)
				}
			}
		})
	}
}
