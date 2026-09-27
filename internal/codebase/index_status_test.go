package codebase

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

func TestSnapshotStatusReportsPreparedReadiness(t *testing.T) {
	_, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{
		"good.lua":    "function Fine()\nend\n",
		"broken.lua":  "function nope(\n",
		"texture.png": pngFixture(8, 4),
	}, "status")
	ctx := context.Background()

	before, err := b.SnapshotStatus(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if before.Ready || before.ReadySnapshots != 0 || before.ActiveSnapshot != pin.ExactCommit {
		t.Fatalf("unprepared status = %+v", before)
	}
	if before.ParserSchema != ParserRevision || before.IndexSchema != indexSchema {
		t.Fatalf("schema fields = %+v", before)
	}

	summary, err := b.IndexSource(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	after, err := b.SnapshotStatus(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if !after.Ready || after.ReadySnapshots != 1 || after.SnapshotFiles != summary.Documents || after.SnapshotFiles != 2 {
		t.Fatalf("ready status = %+v (summary %+v)", after, summary)
	}
	if after.Assets != 1 {
		t.Fatalf("assets = %+v", after)
	}
	if after.ASTFiles != 1 || after.Complete {
		t.Fatalf("syntax accounting = %+v", after)
	}
	if after.Storage != "file-cache" {
		t.Fatalf("storage = %q", after.Storage)
	}
	if info, err := os.Stat(filepath.Join(b.indexPath(pin), "records.jsonl")); err != nil || info.Size() == 0 {
		t.Fatalf("records path = %q %v", b.indexPath(pin), err)
	}
	raw, err := json.Marshal(after)
	if err != nil || string(raw) == "" || containsLegacyDatabaseField(raw) {
		t.Fatalf("status leaked database fields: %s %v", raw, err)
	}

	pinned, err := SourceSnapshotStatus(ctx, b.store.Root(), pinID(t, b, pin))
	if err != nil {
		t.Fatal(err)
	}
	if pinned.Ready != after.Ready {
		t.Fatalf("use case status = %+v", pinned)
	}
}

func containsLegacyDatabaseField(raw []byte) bool {
	var data map[string]any
	if json.Unmarshal(raw, &data) != nil { return true }
	_, database := data["database"]
	_, journal := data["journalMode"]
	return database || journal
}

func pinID(t *testing.T, b *Browser, pin selection.SourcePin) string {
	t.Helper()
	metadata, err := b.store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	pinned, err := selection.OpenPinner(metadata).PinSelection(context.Background(), selection.SelectionSpec{Source: &pin})
	if err != nil {
		t.Fatal(err)
	}
	return pinned.ID
}
