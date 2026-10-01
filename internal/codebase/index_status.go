package codebase

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/selection"
)

// SnapshotStatus reports fixed source mapping readiness. Legacy database fields
// are intentionally absent: a cache directory is not a database.
type SnapshotStatus struct {
	ActiveSnapshot string         `json:"activeSnapshot"`
	ReadySnapshots int            `json:"readySnapshots"`
	SnapshotFiles  int            `json:"snapshotFiles"`
	ASTFiles       int            `json:"astFiles"`
	ParserSchema   string         `json:"parserSchema"`
	IndexSchema    string         `json:"indexSchema"`
	Storage        string         `json:"storage"`
	Assets         int            `json:"assets"`
	Ready          bool           `json:"ready"`
	Complete       bool           `json:"complete"`
	Coverage       SourceCoverage `json:"coverage"`
}

func (b *Browser) SnapshotStatus(ctx context.Context, pin selection.SourcePin) (SnapshotStatus, error) {
	status := SnapshotStatus{ActiveSnapshot: pin.ExactCommit, ParserSchema: ParserRevision, IndexSchema: indexSchema, Storage: "file-cache"}
	cache, summary, err := b.openIndex(ctx, pin)
	if errors.Is(err, errIndexNotReady) {
		return status, nil
	}
	if err != nil {
		return status, err
	}
	defer cache.Close()
	status.Ready = true
	status.ReadySnapshots = 1
	status.SnapshotFiles = summary.Documents + summary.SkippedDocuments
	status.Assets = summary.Assets
	status.Complete = summary.Complete
	status.Coverage = summary.Coverage()
	failed := map[string]bool{}
	analyzed := map[string]bool{}
	err = cache.scanSelected(ctx, func(e recordOffset) bool { return e.Kind == "document" || e.Kind == "diagnostic" }, func(r sourceRecord) error {
		if r.Kind == "document" {
			analyzed[r.Path] = true
		}
		if r.Kind == "diagnostic" {
			failed[r.Path] = true
		}
		return nil
	})
	if err != nil {
		return status, err
	}
	status.ASTFiles = 0
	for name := range analyzed {
		if !failed[name] {
			status.ASTFiles++
		}
	}
	return status, nil
}
