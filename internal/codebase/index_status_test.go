package codebase

import (
	"context"
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
	if after.JournalMode != "delete" {
		t.Fatalf("journal mode = %q", after.JournalMode)
	}
	wantBlobs := filepath.Join(b.store.Root(), "blobs")
	if after.ContentDatabase != wantBlobs {
		t.Fatalf("content database = %q, want %q", after.ContentDatabase, wantBlobs)
	}
	if info, err := os.Stat(after.Database); err != nil || info.Size() == 0 {
		t.Fatalf("database path = %q %v", after.Database, err)
	}

	pinned, err := SourceSnapshotStatus(ctx, b.store.Root(), pinID(t, b, pin))
	if err != nil {
		t.Fatal(err)
	}
	if pinned.Ready != after.Ready {
		t.Fatalf("use case status = %+v", pinned)
	}
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
