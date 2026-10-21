package codebase

import (
	"bytes"
	"context"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func SynchronizeSource(ctx context.Context, root, repository, product, reference string) (selection.PinnedSet, error) {
	s, err := vault.OpenStore(root)
	if err != nil {
		return selection.PinnedSet{}, err
	}
	pin, err := OpenBrowser(s).PrepareSource(ctx, repository, product, reference)
	if err != nil {
		return selection.PinnedSet{}, err
	}
	return vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
	})
}

type SourceReading struct {
	Excerpt SourceExcerpt       `json:"excerpt"`
	Capture evidence.CaptureRef `json:"capture"`
}

func BuildSourceIndex(ctx context.Context, root, snapshot string) (IndexSummary, error) {
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return IndexSummary{}, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return IndexSummary{}, err
	}
	return OpenBrowser(s).IndexSource(ctx, pin)
}

func InspectSource(ctx context.Context, root, snapshot string, query SpanQuery) (SourceReading, error) {
	var result SourceReading
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return result, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	result.Excerpt, err = OpenBrowser(s).ReadSpan(ctx, pin, query)
	if err != nil {
		return result, err
	}
	original, err := s.ReadBlob(ctx, result.Excerpt.Blob, maxSourceBytes)
	if err != nil {
		return result, err
	}
	mediaType := "text/plain; charset=utf-8"
	if len(result.Excerpt.Warnings) > 0 {
		mediaType = "text/plain"
	}
	result.Capture, err = vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(original), MaxBytes: maxSourceBytes, MediaType: mediaType, Complete: true, Provenance: evidence.Provenance{Kind: "source", Locator: pin.Repository + ":" + query.Path, Snapshot: snapshot, SourceCommit: pin.ExactCommit}})
	})
	return result, err
}
