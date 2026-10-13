package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Every item uses the same fixed snapshot and research environment. Pagination
// stays with each item, so a truncated answer can be continued independently.
type BatchRequest struct {
	Requests []BatchItem `json:"requests"`
}

type BatchItem struct {
	ID      string         `json:"id,omitempty"`
	Query   *SearchQuery   `json:"query,omitempty"`
	Refs    *RelationQuery `json:"refs,omitempty"`
	Context *ContextQuery  `json:"context,omitempty"`
}

type BatchItemResult struct {
	ID      string          `json:"id,omitempty"`
	Query   *SearchResponse `json:"query,omitempty"`
	Refs    *RelationResult `json:"refs,omitempty"`
	Context *ContextResult  `json:"context,omitempty"`
}

type BatchResult struct {
	Snapshot  string            `json:"snapshot"`
	Results   []BatchItemResult `json:"results"`
	Complete  bool              `json:"complete"`
	Truncated bool              `json:"truncated"`
}

type SourceBatch struct {
	Result  BatchResult         `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

func (request BatchRequest) validate() error {
	if len(request.Requests) < 1 || len(request.Requests) > 32 {
		return fmt.Errorf("%w: expected 1-32 requests", ErrInvalidBatch)
	}
	for i, item := range request.Requests {
		count := 0
		for _, set := range []bool{item.Query != nil, item.Refs != nil, item.Context != nil} {
			if set {
				count++
			}
		}
		if count != 1 {
			return fmt.Errorf("%w: request %d requires exactly one of query, refs, context", ErrInvalidBatch, i+1)
		}
	}
	return nil
}

func (request BatchRequest) NeedsResearch() bool {
	for _, item := range request.Requests {
		if item.Refs != nil || item.Context != nil {
			return true
		}
	}
	return false
}

func ReadSourceBatch(file string) (BatchRequest, error) {
	var request BatchRequest
	f, err := os.Open(file)
	if err != nil {
		return request, err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, (64<<10)+1))
	if err != nil {
		return request, err
	}
	if len(raw) > 64<<10 {
		return request, fmt.Errorf("%w: request file exceeds 64 KiB", ErrInvalidBatch)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		return request, fmt.Errorf("%w: %v", ErrInvalidBatch, err)
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return request, fmt.Errorf("%w: trailing JSON data", ErrInvalidBatch)
	}
	return request, request.validate()
}

func BatchSource(ctx context.Context, root, snapshot string, request BatchRequest, options ResearchOptions) (SourceBatch, error) {
	ctx, closeQuery := sourceQueryContext(ctx)
	defer closeQuery()
	var reading SourceBatch
	if err := request.validate(); err != nil {
		return reading, err
	}
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return reading, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return reading, err
	}
	b := OpenBrowser(s)
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return reading, err
	}
	reading.Result, err = b.batch(ctx, snapshot, pin, request, options)
	if err != nil {
		return reading, err
	}
	reading.Capture, err = captureSourceJSON(ctx, root, reading.Result, reading.Result.Complete, reading.Result.Truncated, evidence.Provenance{Kind: "source-batch", Locator: "query/refs/context", Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return reading, err
}

func (b *Browser) batch(ctx context.Context, snapshot string, pin selection.SourcePin, request BatchRequest, options ResearchOptions) (BatchResult, error) {
	result := BatchResult{Snapshot: snapshot, Results: []BatchItemResult{}, Complete: true}
	for i, item := range request.Requests {
		row := BatchItemResult{ID: item.ID}
		var complete, truncated bool
		var err error
		switch {
		case item.Query != nil:
			value, queryErr := b.Search(ctx, snapshot, pin, *item.Query)
			row.Query, complete, truncated, err = &value, value.Complete, value.Truncated, queryErr
		case item.Refs != nil:
			value, queryErr := b.Relate(ctx, snapshot, pin, *item.Refs, options)
			row.Refs, complete, truncated, err = &value, value.Complete, value.Truncated, queryErr
		case item.Context != nil:
			value, queryErr := b.Context(ctx, snapshot, pin, *item.Context, options)
			row.Context, complete, truncated, err = &value, value.Complete, value.Truncated, queryErr
		}
		if err != nil {
			return result, fmt.Errorf("source batch request %d (%s): %w", i+1, item.ID, err)
		}
		result.Results = append(result.Results, row)
		result.Complete = result.Complete && complete
		result.Truncated = result.Truncated || truncated
	}
	return result, nil
}
