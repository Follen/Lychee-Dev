package codebase

import (
	"context"
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
	ctx, closeQuery := sourceQueryContext(ctx)
	defer closeQuery()

	var result SourceComparison
	pair, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, m *vault.Metadata) (SourcePair, error) {
		pinner := selection.OpenPinner(m)
		left, err := pinner.ReadPinnedSet(ctx, from)
		if err != nil {
			return SourcePair{}, err
		}
		right, err := pinner.ReadPinnedSet(ctx, to)
		if err != nil {
			return SourcePair{}, err
		}
		if left.Source == nil || right.Source == nil {
			return SourcePair{}, errors.New("codebase.source_pin_required")
		}
		return SourcePair{From: *left.Source, To: *right.Source}, nil
	})
	if err != nil {
		return result, err
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	browser := OpenBrowser(store)
	query := ChangeQuery{Limit: limit}
	result.Result, err = browser.CompareTrees(ctx, pair, query)
	if errors.Is(err, errIndexNotReady) {
		// CompareTrees owns pair validation and incomplete-map rejection. Only
		// missing mappings need preparation, using the already-fixed commits and
		// the shared request cache, outside the metadata publication lease.
		for _, pin := range []selection.SourcePin{pair.From, pair.To} {
			if err = browser.EnsureIndex(ctx, pin); err != nil {
				return result, err
			}
		}
		result.Result, err = browser.CompareTrees(ctx, pair, query)
	}
	if err != nil {
		return result, err
	}
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, !result.Result.Truncated, result.Result.Truncated,
		evidence.Provenance{Kind: "source-diff", Locator: from + ".." + to, Snapshot: to, SourceCommit: pair.To.ExactCommit, BaseSnapshot: from, BaseSourceCommit: pair.From.ExactCommit})
	return result, err
}
