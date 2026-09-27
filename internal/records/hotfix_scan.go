package records

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

// HotfixScan is a bounded continuation over immutable cache pages. Page captures
// contain the records; this manifest records cumulative coverage and resumption.
type HotfixScan struct {
	Snapshot  string              `json:"snapshot"`
	Source    evidence.CaptureRef `json:"source"`
	Query     HotfixRequest       `json:"query"`
	Pages     []string            `json:"pages"`
	Matched   uint32              `json:"matched"`
	Returned  uint32              `json:"returned"`
	Decoded   uint32              `json:"decoded"`
	NoPayload uint32              `json:"noPayload"`
	NotValid  uint32              `json:"notValid"`
	Raw       uint32              `json:"raw"`
	NextIndex *uint32             `json:"nextIndex,omitempty"`
	Complete  bool                `json:"complete"`
	Truncated bool                `json:"truncated"`
}
type HotfixScanReading struct {
	Result  HotfixScan          `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
	Resume  string              `json:"resume,omitempty"`
}

func ScanHotfix(ctx context.Context, root, snapshot string, q HotfixRequest, resume string, maxPages int) (HotfixScanReading, error) {
	return scanHotfix(ctx, root, snapshot, q, resume, maxPages, InspectHotfix)
}

func scanHotfix(ctx context.Context, root, snapshot string, q HotfixRequest, resume string, maxPages int, inspect func(context.Context, string, string, HotfixRequest) (HotfixReading, error)) (reading HotfixScanReading, err error) {
	var checkpoint HotfixScanReading
	defer func() {
		if err != nil && checkpoint.Capture.ID != "" {
			reading = checkpoint
		}
	}()
	if err := q.Validate(); err != nil {
		return HotfixScanReading{}, err
	}
	if q.Filter.AfterIndex != nil || maxPages < 1 || maxPages > 1000 {
		return HotfixScanReading{}, ErrCacheLimit
	}
	state := HotfixScan{Snapshot: snapshot, Query: q, Pages: []string{}}
	if resume != "" {
		var previousRef evidence.CaptureRef
		if q.File != "" {
			return HotfixScanReading{}, ErrCacheIdentity
		}
		previous, err := vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (HotfixScan, error) {
			ref, raw, err := evidence.OpenArchive(s, m).FetchCapture(ctx, resume, 4<<20)
			if err != nil {
				return HotfixScan{}, err
			}
			var saved HotfixScan
			if ref.Provenance.Kind != "hotfix-scan" || json.Unmarshal(raw, &saved) != nil || saved.Snapshot != snapshot || saved.Source.ID != q.From {
				return HotfixScan{}, ErrCacheIdentity
			}
			a, b := saved.Query, q
			a.Offline, b.Offline = false, false
			x, _ := json.Marshal(a)
			y, _ := json.Marshal(b)
			if !bytes.Equal(x, y) {
				return HotfixScan{}, fmt.Errorf("%w: resume filters changed", ErrCacheIdentity)
			}
			previousRef = ref
			return saved, nil
		})
		if err != nil {
			return HotfixScanReading{}, err
		}
		state = previous
		checkpoint = HotfixScanReading{Result: state, Capture: previousRef, Resume: previousRef.ID}
		if state.Complete {
			return HotfixScanReading{Result: state, Capture: previousRef}, nil
		}
	}
	var last evidence.CaptureRef
	for pageNumber := 0; pageNumber < maxPages; pageNumber++ {
		if len(state.Pages) >= 10000 {
			return HotfixScanReading{}, ErrCacheLimit
		}
		request := state.Query
		request.Offline = q.Offline
		request.Filter.AfterIndex = state.NextIndex
		page, err := inspect(ctx, root, state.Snapshot, request)
		if err != nil {
			return HotfixScanReading{}, err
		}
		result := page.Result
		if len(state.Pages) > 0 && (result.Source.ID != state.Source.ID || result.Page.Matched != state.Matched) {
			return HotfixScanReading{}, ErrCacheIdentity
		}
		state.Snapshot, state.Source, state.Matched = result.Snapshot, result.Source, result.Page.Matched
		state.Query.File = ""
		state.Query.From = result.Source.ID
		for _, entry := range result.Page.Entries {
			if state.NextIndex != nil && entry.Index <= *state.NextIndex {
				return HotfixScanReading{}, ErrHotfixCursor
			}
			index := entry.Index
			state.NextIndex = &index
			state.Returned++
			switch entry.DecodeState {
			case "decoded":
				state.Decoded++
			case "no_payload":
				state.NoPayload++
			case "not_valid":
				state.NotValid++
			default:
				state.Raw++
			}
		}
		state.Pages = append(state.Pages, page.Capture.ID)
		state.Truncated = result.Page.Truncated
		state.Complete = !state.Truncated && state.Returned == state.Matched
		if !state.Truncated && !state.Complete {
			return HotfixScanReading{}, ErrHotfixCursor
		}
		if state.Truncated && (result.Page.NextIndex == nil || state.NextIndex == nil || *result.Page.NextIndex != *state.NextIndex) {
			return HotfixScanReading{}, ErrHotfixCursor
		}
		if state.Complete {
			state.NextIndex = nil
		}
		last, err = vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
			raw, err := json.Marshal(state)
			if err != nil {
				return evidence.CaptureRef{}, err
			}
			return evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4 << 20, MediaType: "application/json", Complete: state.Complete, Truncated: state.Truncated, Provenance: evidence.Provenance{Kind: "hotfix-scan", Locator: "capture:" + state.Source.ID, Snapshot: state.Snapshot, DataBuild: state.Source.Provenance.DataBuild}})
		})
		if err != nil {
			return HotfixScanReading{}, err
		}
		checkpoint = HotfixScanReading{Result: state, Capture: last, Resume: last.ID}
		if state.Complete {
			break
		}
	}
	reading = HotfixScanReading{Result: state, Capture: last}
	if !state.Complete {
		reading.Resume = last.ID
	}
	return reading, nil
}
