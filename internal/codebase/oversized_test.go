package codebase

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestOversizedLuaDocumentDoesNotBlockOtherSource(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "Main.lua"), []byte("function Usable() end\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	large, err := os.Create(filepath.Join(root, "ModelPaths.lua"))
	if err != nil {
		t.Fatal(err)
	}
	if err := large.Truncate(maxSourceBytes + 1); err != nil {
		large.Close()
		t.Fatal(err)
	}
	if err := large.Close(); err != nil {
		t.Fatal(err)
	}
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", root)
	if err != nil {
		t.Fatal(err)
	}
	if indexed.Summary.Documents != 1 || indexed.Summary.SkippedDocuments != 1 || indexed.Summary.Complete || len(indexed.Summary.DiagnosticSample) != 1 || !strings.Contains(indexed.Summary.DiagnosticSample[0].Message, "skipped") {
		t.Fatalf("summary = %+v", indexed.Summary)
	}
	reading, err := QuerySource(ctx, store.Root(), indexed.Snapshot.ID, SearchQuery{Mode: SearchModePrecise, Text: "Usable"})
	if err != nil {
		t.Fatal(err)
	}
	if len(reading.Result.Results) == 0 || reading.Result.Results[0].Name != "Usable" {
		t.Fatalf("query = %+v", reading.Result)
	}
	if reading.Result.Coverage.SkippedDocuments != 1 || reading.Result.Coverage.DiagnosticSample[0].Path != "ModelPaths.lua" {
		t.Fatalf("query coverage = %+v", reading.Result.Coverage)
	}
	status, err := OpenBrowser(store).SnapshotStatus(ctx, indexed.Pin)
	if err != nil {
		t.Fatal(err)
	}
	if status.SnapshotFiles != 2 || status.ASTFiles != 1 || status.Coverage.SkippedDocuments != 1 {
		t.Fatalf("status = %+v", status)
	}
}

func TestLuaDocumentAbovePreviousLimitIsIndexed(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	data := []byte("--" + strings.Repeat("x", 16<<20) + "\nfunction LargeSourceSymbol() end\n")
	if err := os.WriteFile(filepath.Join(root, "Large.lua"), data, 0o644); err != nil {
		t.Fatal(err)
	}
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", root)
	if err != nil {
		t.Fatal(err)
	}
	if indexed.Summary.Documents != 1 || indexed.Summary.SkippedDocuments != 0 || !indexed.Summary.Complete {
		t.Fatalf("summary = %+v", indexed.Summary)
	}
	reading, err := QuerySource(ctx, store.Root(), indexed.Snapshot.ID, SearchQuery{Mode: SearchModePrecise, Text: "LargeSourceSymbol"})
	if err != nil {
		t.Fatal(err)
	}
	if len(reading.Result.Results) == 0 || reading.Result.Results[0].Name != "LargeSourceSymbol" {
		t.Fatalf("query = %+v", reading.Result)
	}
}
