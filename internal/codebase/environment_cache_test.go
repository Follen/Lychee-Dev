package codebase

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestResearchEnvironmentCacheColdHotAndCorruptRebuild(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	first, state, err := b.researchEnvironment(ctx, indexed.Pin, "")
	if err != nil || state != "ready" || len(first.Definitions) == 0 || len(first.Facts) == 0 {
		t.Fatalf("cold: state=%s err=%v result=%+v", state, err, first.Manifest)
	}
	hot, state, err := b.researchEnvironment(ctx, indexed.Pin, "")
	if err != nil || state != "ready" || hot.Manifest.InputSHA256 != first.Manifest.InputSHA256 || hot.Manifest.DefinitionsSHA256 != first.Manifest.DefinitionsSHA256 {
		t.Fatalf("hot: state=%s err=%v", state, err)
	}
	parent := filepath.Join(store.Root(), "source", "v1", "environments")
	entries, err := os.ReadDir(parent)
	if err != nil || len(entries) != 1 {
		t.Fatalf("cache dirs=%v err=%v", entries, err)
	}
	dir := filepath.Join(parent, entries[0].Name())
	if err := os.WriteFile(filepath.Join(dir, "manifest.json"), []byte("{corrupt"), 0o644); err != nil {
		t.Fatal(err)
	}
	rebuilt, state, err := b.researchEnvironment(ctx, indexed.Pin, "")
	if err != nil || state != "ready" || rebuilt.Manifest.InputSHA256 != first.Manifest.InputSHA256 || rebuilt.Manifest.DefinitionsSHA256 != first.Manifest.DefinitionsSHA256 {
		t.Fatalf("rebuilt: state=%s err=%v", state, err)
	}
	if _, err := os.Stat(filepath.Join(dir, "api.d.lua")); err != nil {
		t.Fatal(err)
	}
	if err := os.Truncate(filepath.Join(dir, "facts.jsonl"), maxEnvironmentFactsBytes+1); err != nil {
		t.Fatal(err)
	}
	bounded, state, err := b.researchEnvironment(ctx, indexed.Pin, "")
	if err != nil || state != "ready" || bounded.Manifest.InputSHA256 != first.Manifest.InputSHA256 {
		t.Fatalf("oversized corrupt cache: state=%s err=%v", state, err)
	}
}
