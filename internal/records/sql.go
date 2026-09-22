package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"strings"
)

type DataQuery struct {
	SQL        string         `json:"sql"`
	Parameters map[string]any `json:"parameters,omitempty"`
}
type QueryResult struct {
	Query   DataQuery         `json:"query"`
	Result  relational.Result `json:"result"`
	Sources []TableReading    `json:"sources"`
}
type QueryReading struct {
	Result  QueryResult
	Capture evidence.CaptureRef
}

// QueryData binds every physical table to the same DataPin and selected source.
// Hotfix and CDN are never silently substituted for unavailable local data.
func QueryData(ctx context.Context, root, snapshot string, files FileQuery, request DataQuery) (QueryReading, error) {
	if files.ContentBytes < 1 || files.ContentBytes > 512<<20 {
		return QueryReading{}, relational.ErrBudget
	}
	program, err := relational.Compile(ctx, request.SQL)
	if err != nil {
		return QueryReading{}, err
	}
	if len(program.SourceTables()) > 32 {
		return QueryReading{}, relational.ErrBudget
	}
	names := program.NamedParameters()
	if len(names) != len(request.Parameters) {
		return QueryReading{}, relational.ErrBinding
	}
	for _, name := range names {
		if _, ok := request.Parameters[name]; !ok {
			return QueryReading{}, relational.ErrBinding
		}
	}
	// Own a bounded JSON snapshot for execution and evidence. UseNumber keeps
	// integer parameters exact across the serialization seam.
	encoded, err := json.Marshal(request)
	if err != nil {
		return QueryReading{}, err
	}
	if len(encoded) > 1<<20 {
		return QueryReading{}, relational.ErrBudget
	}
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.UseNumber()
	if err := decoder.Decode(&request); err != nil {
		return QueryReading{}, err
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (QueryReading, error) {
		pin, err := selection.OpenPinner(metadata).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return QueryReading{}, err
		}
		if pin.Data == nil {
			return QueryReading{}, errors.New("records.data_pin_required")
		}
		if _, _, err := selection.DataIdentity(*pin.Data); err != nil {
			return QueryReading{}, err
		}
		definitions := OpenDefinitions(store, metadata)
		reader := OpenReader(store)
		reading := QueryResult{Query: request, Sources: []TableReading{}}
		remaining := int64(512 << 20)
		resolve := func(ctx context.Context, use relational.TableUse) (relational.Source, error) {
			if !strings.EqualFold(use.Catalog, "static") {
				return relational.Source{}, relational.ErrUnsupported
			}
			if remaining <= 0 || len(reading.Sources) >= 32 {
				return relational.Source{}, relational.ErrBudget
			}
			bundle, err := definitions.Prepare(ctx, pin.Data.DefinitionCommit, use.Name, files.Offline)
			if err != nil {
				return relational.Source{}, err
			}
			query := files
			query.FileDataID, query.ContentBytes = bundle.Identity.DB2FileDataID, min(files.ContentBytes, remaining)
			view, provenance, err := reader.PrepareTable(ctx, *pin.Data, query, bundle)
			if err != nil {
				return relational.Source{}, err
			}
			remaining -= provenance.File.Content.Bytes
			source, err := TableSource(view, 4<<20)
			if err != nil {
				return relational.Source{}, err
			}
			reading.Sources = append(reading.Sources, provenance)
			return source, nil
		}
		reading.Result, err = program.Execute(ctx, resolve, request.Parameters, relational.Limits{Work: 10000000, MemoryBytes: 64 << 20})
		if err != nil {
			return QueryReading{}, err
		}
		raw, err := json.Marshal(reading)
		if err != nil {
			return QueryReading{}, err
		}
		if len(raw) > 16<<20 {
			return QueryReading{}, relational.ErrBudget
		}
		digest := sha256.Sum256(encoded)
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: true,
			Provenance: evidence.Provenance{Kind: "data-query", Locator: "sql:sha256:" + hex.EncodeToString(digest[:]), Snapshot: snapshot, DataBuild: pin.Data.FullBuild},
		})
		if err != nil {
			return QueryReading{}, err
		}
		return QueryReading{Result: reading, Capture: capture}, nil
	})
}
