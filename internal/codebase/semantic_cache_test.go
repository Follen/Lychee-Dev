package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSemanticCacheBindsRuntimeEnvironmentAndSource(t *testing.T) {
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	pin := selection.SourcePin{Repository: "wow-ui-source", Product: "retail", ExactCommit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
	env := environment.Manifest{Identity: environment.Identity{Repository: "wow-ui-source", Commit: pin.ExactCommit}, InputSHA256: "input", DefinitionsSHA256: "definitions"}
	runtime := &luals.Runtime{Identity: luals.Identity{Version: "3.19.1", SHA256: "runtime-a"}}
	symbol := SymbolMatch{ID: "SYM-1", Name: "Foo"}
	calls := 0
	compute := func() ([]ResearchRelation, string) {
		calls++
		return []ResearchRelation{{Source: "x", Target: "Foo", Kind: "reference", State: "resolved", Path: "x.lua", Line: 1}}, ""
	}
	for i := 0; i < 2; i++ {
		rows, reason := b.cachedSemantic(context.Background(), pin, "records-a", env, runtime, symbol, "incoming", compute)
		if reason != "" || len(rows) != 1 {
			t.Fatalf("cached rows=%+v reason=%s", rows, reason)
		}
	}
	if calls != 1 {
		t.Fatalf("hot cache recomputed %d times", calls)
	}
	runtime.Identity.SHA256 = "runtime-b"
	b.cachedSemantic(context.Background(), pin, "records-a", env, runtime, symbol, "incoming", compute)
	env.InputSHA256 = "new-input"
	b.cachedSemantic(context.Background(), pin, "records-a", env, runtime, symbol, "incoming", compute)
	b.cachedSemantic(context.Background(), pin, "records-b", env, runtime, symbol, "incoming", compute)
	if calls != 4 {
		t.Fatalf("identity changes reused cache: %d", calls)
	}
}

func TestSemanticCacheRejectsOldEmptyCompleteResult(t *testing.T) {
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	pin := selection.SourcePin{Repository: "wow-ui-source", Product: "retail", ExactCommit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
	env := environment.Manifest{Identity: environment.Identity{Repository: "wow-ui-source", Commit: pin.ExactCommit}, InputSHA256: "input", DefinitionsSHA256: "definitions"}
	runtime := &luals.Runtime{Identity: luals.Identity{Version: "3.19.1", SHA256: "runtime-a"}}
	symbol := SymbolMatch{ID: "SYM-1", Name: "UnitHealth"}
	oldSchema := "lycheedev.source-semantic.v1"
	if semanticCacheSchema == oldSchema {
		t.Fatal("adapter fix did not invalidate old cache namespace")
	}
	parts, err := json.Marshal([]string{oldSchema, pin.Repository, pin.Product, pin.ExactCommit, "records", env.Identity.Repository, env.Identity.Commit, env.InputSHA256, env.DefinitionsSHA256, runtime.Identity.Version, runtime.Identity.SHA256, symbol.ID, "incoming"})
	if err != nil {
		t.Fatal(err)
	}
	keyHash := sha256.Sum256(parts)
	key := hex.EncodeToString(keyHash[:])
	rowsHash := sha256.Sum256([]byte("[]"))
	old, err := json.Marshal(semanticCacheRecord{Schema: oldSchema, Key: key, RowsHash: hex.EncodeToString(rowsHash[:]), Rows: []ResearchRelation{}})
	if err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(store.Root(), "source", "v1", "semantics")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, key+".json"), old, 0o644); err != nil {
		t.Fatal(err)
	}
	calls := 0
	rows, reason := b.cachedSemantic(context.Background(), pin, "records", env, runtime, symbol, "incoming", func() ([]ResearchRelation, string) {
		calls++
		return []ResearchRelation{{Source: "probe.lua", Target: "UnitHealth", State: "resolved", Kind: "reference", Path: "probe.lua", Line: 2}}, ""
	})
	if reason != "" || calls != 1 || len(rows) != 1 {
		t.Fatalf("old empty cache reused: calls=%d rows=%+v reason=%s", calls, rows, reason)
	}
}
