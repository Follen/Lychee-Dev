package codebase

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSyntheticCommitIDFrozenRule(t *testing.T) {
	// These values freeze the documented rule. Changing them silently would
	// re-identify every fixture index and pin, so they are asserted exactly.
	cases := map[string]string{
		`C:\fixture\tree`: "0000000000000000000000004a9d94515dba9615",
		"sample/fixture":  "000000000000000000000000cb79330de99080af",
	}
	for input, want := range cases {
		got := SyntheticCommitID(input)
		if got != want {
			t.Fatalf("SyntheticCommitID(%q) = %q, want %q", input, got, want)
		}
		if len(got) != 40 || got != strings.ToLower(got) {
			t.Fatalf("synthetic id shape = %q", got)
		}
		if repeat := SyntheticCommitID(input); repeat != got {
			t.Fatalf("synthetic id unstable: %q vs %q", got, repeat)
		}
	}
	if SyntheticCommitID("a") == SyntheticCommitID("b") {
		t.Fatal("different paths share one synthetic id")
	}
}

func TestIndexFixtureSourceDeterministicAndQueryable(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	home := store.Root()
	fixturePath := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")

	first, err := IndexFixtureSource(ctx, home, "wow-ui-source", "retail", fixturePath)
	if err != nil {
		t.Fatal(err)
	}
	absolute, err := filepath.Abs(fixturePath)
	if err != nil {
		t.Fatal(err)
	}
	if first.Pin.ExactCommit != SyntheticCommitID(absolute) || first.Summary.Documents != 2 {
		t.Fatalf("fixture index identity = %+v / %+v", first.Pin, first.Summary)
	}
	if first.Snapshot.ID == "" || first.Snapshot.Source == nil || first.Snapshot.Source.ExactCommit != first.Pin.ExactCommit {
		t.Fatalf("fixture pin not persisted: %+v", first.Snapshot)
	}
	second, err := IndexFixtureSource(ctx, home, "wow-ui-source", "retail", fixturePath)
	if err != nil {
		t.Fatal(err)
	}
	if second.Pin != first.Pin || second.Summary.Documents != first.Summary.Documents {
		t.Fatalf("fixture index is not deterministic: %+v vs %+v", first, second)
	}

	reading, err := QuerySource(ctx, home, first.Snapshot.ID, SearchQuery{Mode: SearchModePrecise, Text: "GetItemSearchResultInfo"})
	if err != nil {
		t.Fatal(err)
	}
	if len(reading.Result.Results) == 0 {
		t.Fatalf("fixture symbol not found: %+v", reading.Result)
	}
	match := reading.Result.Results[0]
	if match.Name != "C_AuctionHouse.GetItemSearchResultInfo" || match.MatchedBy != "exact_symbol" || match.ContentHash == "" || match.Excerpt == "" {
		t.Fatalf("fixture symbol row = %+v", match)
	}
	if !strings.HasSuffix(match.Path, "GeneratedDocumentation.lua") {
		t.Fatalf("fixture symbol path = %q", match.Path)
	}
}

func TestFixtureSourcePinRequiresCatalogIdentity(t *testing.T) {
	if _, err := FixtureSourcePin("unknown-repo", "retail", t.TempDir()); err == nil || !strings.Contains(err.Error(), "codebase.repository_not_found") {
		t.Fatalf("unknown repository accepted: %v", err)
	}
	if _, err := FixtureSourcePin("wow-ui-source", "unknown-product", t.TempDir()); err == nil || !strings.Contains(err.Error(), "codebase.unknown_product") {
		t.Fatalf("unknown product accepted: %v", err)
	}
}
