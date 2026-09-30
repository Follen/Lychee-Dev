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
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"strings"
	"time"
)

type DataQuery struct {
	Hotfix     []string       `json:"hotfix,omitempty"`
	SQL        string         `json:"sql"`
	Parameters map[string]any `json:"parameters,omitempty"`
}
type QueryResult struct {
	Resources        resource.Usage    `json:"resources"`
	EffectiveSources []EffectiveTable  `json:"effectiveSources,omitempty"`
	Timings          QueryTimings      `json:"timings"`
	SemanticSources  []SemanticBundle  `json:"semanticSources,omitempty"`
	Complete         bool              `json:"complete"`
	Partial          bool              `json:"partial"`
	Query            DataQuery         `json:"query"`
	Result           relational.Result `json:"result"`
	Sources          []TableReading    `json:"sources"`
}

// Timings are wall-clock observations, not optimizer estimates or peak memory.
type QueryTimings struct {
	DefinitionsMS      float64 `json:"definitionsMs"`
	PreparationMS      float64 `json:"preparationMs"`
	ExecutionMS        float64 `json:"executionMs"`
	DecodedSourceBytes int64   `json:"decodedSourceBytes"`
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
	if len(request.Hotfix) > 16 {
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
		files = ensureQueryBudget(files)
		admission, err := store.AcquireDataResources(ctx, vault.DataOrdinary)
		if err != nil {
			return QueryReading{}, err
		}
		defer admission.Close()
		files.admitted = true
		// Reserve the executor's independently enforced memory limit inside
		// the whole-query retained budget before any source is prepared.
		if err := files.budget.Charge(resource.Cost{RetainedBytes: 64 << 20}); err != nil {
			return QueryReading{}, err
		}
		definitions := OpenDefinitions(store, metadata).WithBudget(files.budget)
		reader, err := OpenReader(store).NewSession(ctx, *pin.Data, files)
		if err != nil {
			return QueryReading{}, err
		}
		defer reader.Close()
		files = reader.Query()
		reading := QueryResult{Query: request, Sources: []TableReading{}, Complete: true}
		remaining := int64(512 << 20)
		resolve := func(ctx context.Context, use relational.TableUse) (relational.Source, error) {
			started := time.Now()
			if strings.EqualFold(use.Catalog, "meta") {
				source, provenance, err := definitions.SemanticSource(ctx, pin.Data.DefinitionCommit, pin.Data.FullBuild, use.Name, files.Offline)
				if err == nil {
					reading.SemanticSources = append(reading.SemanticSources, provenance)
				}
				reading.Timings.DefinitionsMS += float64(time.Since(started)) / float64(time.Millisecond)
				return source, err
			}
			effective := strings.EqualFold(use.Catalog, "effective")
			if !strings.EqualFold(use.Catalog, "static") && !effective {
				return relational.Source{}, relational.ErrUnsupported
			}
			if remaining <= 0 || len(reading.Sources) >= 32 {
				return relational.Source{}, relational.ErrBudget
			}
			bundle, err := definitions.Prepare(ctx, pin.Data.DefinitionCommit, use.Name, files.Offline)
			reading.Timings.DefinitionsMS += float64(time.Since(started)) / float64(time.Millisecond)
			if err != nil {
				return relational.Source{}, err
			}
			query := files
			query.FileDataID, query.ContentBytes = bundle.Identity.DB2FileDataID, min(files.ContentBytes, remaining)
			started = time.Now()
			view, provenance, err := reader.PrepareTable(ctx, *pin.Data, query, bundle)
			reading.Timings.PreparationMS += float64(time.Since(started)) / float64(time.Millisecond)
			if err != nil {
				return relational.Source{}, err
			}
			remaining -= provenance.File.Content.Bytes
			reading.Timings.DecodedSourceBytes += provenance.File.Content.Bytes
			if provenance.File.PartialContent != nil {
				remaining -= provenance.File.PartialContent.Bytes
				reading.Timings.DecodedSourceBytes += provenance.File.PartialContent.Bytes
				reading.Complete, reading.Partial = false, true
			}
			source, err := TableSource(view, 4<<20)
			if err != nil {
				return relational.Source{}, err
			}
			if effective {
				var overlay EffectiveTable
				source, overlay, err = reader.EffectiveSource(ctx, store, metadata, *pin.Data, source, view.Definition(), bundle, request.Hotfix)
				if err != nil {
					return relational.Source{}, err
				}
				reading.EffectiveSources = append(reading.EffectiveSources, overlay)
			}
			reading.Sources = append(reading.Sources, provenance)
			return source, nil
		}
		started := time.Now()
		reading.Result, err = program.Execute(ctx, resolve, request.Parameters, relational.Limits{Work: 10000000, MemoryBytes: 64 << 20})
		reading.Timings.ExecutionMS = max(0, float64(time.Since(started))/float64(time.Millisecond)-reading.Timings.DefinitionsMS-reading.Timings.PreparationMS)
		if err != nil {
			return QueryReading{}, err
		}
		if err := reader.CheckSource(ctx); err != nil {
			return QueryReading{}, err
		}
		reading.Resources = files.budget.Snapshot()
		raw, err := json.Marshal(reading)
		if err != nil {
			return QueryReading{}, err
		}
		if len(raw) > 16<<20 {
			return QueryReading{}, relational.ErrBudget
		}
		digest := sha256.Sum256(encoded)
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: reading.Complete,
			Provenance: evidence.Provenance{Kind: "data-query", Locator: "sql:sha256:" + hex.EncodeToString(digest[:]), Snapshot: snapshot, DataBuild: pin.Data.FullBuild},
		})
		if err != nil {
			return QueryReading{}, err
		}
		return QueryReading{Result: reading, Capture: capture}, nil
	})
}
