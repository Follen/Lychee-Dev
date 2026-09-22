package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type SourceComparison struct {
	Result  SourceDelta
	Capture evidence.CaptureRef
}

func CompareSource(ctx context.Context, root, from, to string, limit int) (SourceComparison, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (SourceComparison, error) {
		var result SourceComparison
		pinner := selection.OpenPinner(m)
		left, err := pinner.ReadPinnedSet(ctx, from)
		if err != nil {
			return result, err
		}
		right, err := pinner.ReadPinnedSet(ctx, to)
		if err != nil {
			return result, err
		}
		if left.Source == nil || right.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenBrowser(s).CompareTrees(ctx, SourcePair{From: *left.Source, To: *right.Source}, ChangeQuery{Limit: limit})
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: !result.Result.Truncated, Truncated: result.Result.Truncated,
			Provenance: evidence.Provenance{Kind: "source-diff", Locator: from + ".." + to, Snapshot: to, SourceCommit: right.Source.ExactCommit, BaseSnapshot: from, BaseSourceCommit: left.Source.ExactCommit},
		})
		return result, err
	})
}
