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

func SynchronizeSource(ctx context.Context, root, repository, product, reference string) (selection.PinnedSet, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
		pin, err := OpenBrowser(s).PrepareSource(ctx, repository, product, reference)
		if err != nil {
			return selection.PinnedSet{}, err
		}
		return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
	})
}

type SourceReading struct {
	Excerpt SourceExcerpt       `json:"excerpt"`
	Capture evidence.CaptureRef `json:"capture"`
}

func BuildSourceIndex(ctx context.Context, root, snapshot string) (IndexSummary, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (IndexSummary, error) {
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return IndexSummary{}, err
		}
		if pin.Source == nil {
			return IndexSummary{}, errors.New("codebase.source_pin_required")
		}
		return OpenBrowser(s).IndexSource(ctx, *pin.Source)
	})
}

type SourceSearch struct {
	Result  SymbolMatches
	Capture evidence.CaptureRef
}

func SearchSource(ctx context.Context, root, snapshot, term string, limit int) (SourceSearch, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (SourceSearch, error) {
		var result SourceSearch
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenBrowser(s).FindSymbols(ctx, *pin.Source, term, limit)
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: result.Result.Index.Complete && !result.Result.Truncated, Truncated: result.Result.Truncated,
			Provenance: evidence.Provenance{Kind: "source-query", Locator: term, Snapshot: snapshot, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
}

func InspectSource(ctx context.Context, root, snapshot string, query SpanQuery) (SourceReading, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (SourceReading, error) {
		var result SourceReading
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Excerpt, err = OpenBrowser(s).ReadSpan(ctx, *pin.Source, query)
		if err != nil {
			return result, err
		}
		original, err := s.ReadBlob(ctx, result.Excerpt.Blob, 16<<20)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(original), MaxBytes: 16 << 20, MediaType: "text/plain; charset=utf-8", Complete: true,
			Provenance: evidence.Provenance{Kind: "source", Locator: pin.Source.Repository + ":" + query.Path, Snapshot: pin.ID, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
}
