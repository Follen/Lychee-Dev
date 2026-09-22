package codebase

import (
	"context"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/selection"
)

// SnapshotStatus is the `index status` read model of one pinned snapshot. The
// field names stay stable for callers; values describe this workspace's actual
// storage (journalMode reports the real SQLite mode).
type SnapshotStatus struct {
	ActiveSnapshot  string `json:"activeSnapshot"`
	ReadySnapshots  int    `json:"readySnapshots"`
	SnapshotFiles   int    `json:"snapshotFiles"`
	ASTFiles        int    `json:"astFiles"`
	ParserSchema    string `json:"parserSchema"`
	IndexSchema     string `json:"indexSchema"`
	Database        string `json:"database"`
	ContentDatabase string `json:"contentDatabase"`
	JournalMode     string `json:"journalMode"`
	Assets          int    `json:"assets"`
	// Ready reports prepared-snapshot readiness for source list output, and
	// Complete reports whether the index finished without syntax diagnostics.
	Ready    bool `json:"ready"`
	Complete bool `json:"complete"`
}

// SnapshotStatus reports index readiness for one pinned source without
// building anything. An absent index is a readiness answer, not an error.
func (b *Browser) SnapshotStatus(ctx context.Context, pin selection.SourcePin) (SnapshotStatus, error) {
	status := SnapshotStatus{
		ActiveSnapshot:  pin.ExactCommit,
		ParserSchema:    ParserRevision,
		IndexSchema:     indexSchema,
		Database:        b.indexPath(pin),
		ContentDatabase: filepath.Join(b.store.Root(), "blobs"),
	}
	db, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		if errors.Is(err, errIndexNotReady) {
			return status, nil
		}
		return status, err
	}
	defer db.Close()
	status.Ready = true
	status.ReadySnapshots = 1
	status.SnapshotFiles = summary.Documents
	status.Assets = summary.Assets
	status.Complete = summary.Complete
	status.IndexSchema = summary.Schema
	status.Database = b.indexPathFor(summary.Schema, pin)
	var failed int
	if err := db.QueryRowContext(ctx, "SELECT COUNT(DISTINCT path) FROM diagnostics").Scan(&failed); err != nil {
		return status, err
	}
	status.ASTFiles = summary.Documents - failed
	var journal string
	if err := db.QueryRowContext(ctx, "PRAGMA journal_mode").Scan(&journal); err != nil {
		return status, err
	}
	status.JournalMode = journal
	return status, nil
}
