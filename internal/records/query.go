package records

import (
	"bytes"
	"context"
	"errors"
	"fmt"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/selection"
)

type TableReading struct {
	Complete              bool                   `json:"complete"`
	Truncated             bool                   `json:"truncated"`
	UnavailablePartitions []UnavailablePartition `json:"unavailablePartitions,omitempty"`
	File                  FileReading            `json:"file"`
	Sources               DefinitionBundle       `json:"sources"`
	LayoutHash            string                 `json:"layoutHash"`
	Schema                schema.Definition      `json:"schema"`
	RecordID              *uint32                `json:"recordID,omitempty"`
	Row                   map[string]any         `json:"row,omitempty"`
	Page                  *table.RowPage         `json:"page,omitempty"`
}

// ReadRecord consumes only authenticated definition blobs from the shared
// vault, rebinds their table identity to the actual WDC header and selects an
// exact layout. No Hotfix overlay or latest-definition substitution is involved.
func (r *Reader) ReadRecord(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle, id uint32) (TableReading, error) {
	return r.readSelection(ctx, pin, q, bundle, &id, nil, 0)
}

func (r *Reader) ReadPage(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle, after *uint32, limit int) (TableReading, error) {
	if limit < 1 || limit > 200 {
		return TableReading{}, table.ErrLimit
	}
	return r.readSelection(ctx, pin, q, bundle, nil, after, limit)
}

func (r *Reader) readSelection(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle, id, after *uint32, limit int) (TableReading, error) {
	return r.withTableView(ctx, pin, q, bundle, func(view *table.View, result TableReading) (TableReading, error) {
		if id != nil {
			value := *id
			row, err := view.Row(ctx, value, 1<<20)
			if errors.Is(err, table.ErrRecordMissing) && !result.Complete {
				key, unknown := unavailableRecord(result.UnavailablePartitions, value)
				if key != "" {
					return TableReading{}, fmt.Errorf("%w: record %d belongs to encrypted key %s", container.ErrKeyUnavailable, value, key)
				}
				if unknown {
					return TableReading{}, fmt.Errorf("%w: record %d has unknown presence in unavailable partitions", container.ErrKeyUnavailable, value)
				}
			}
			if err != nil {
				return TableReading{}, err
			}
			result.RecordID, result.Row = &value, row
		} else {
			page, err := view.Page(ctx, after, limit, 8<<20)
			if err != nil {
				return TableReading{}, err
			}
			result.Page = &page
			result.Truncated = page.More
			result.Complete = result.Complete && page.After == nil && !page.More
		}
		return result, nil
	})
}

func (r *Reader) withTableView(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle, consume func(*table.View, TableReading) (TableReading, error)) (TableReading, error) {
	if err := ctx.Err(); err != nil {
		return TableReading{}, err
	}
	if _, _, err := selection.DataIdentity(pin); err != nil {
		return TableReading{}, err
	}
	if r.store == nil || bundle.Commit != pin.DefinitionCommit {
		return TableReading{}, ErrDefinitionIdentity
	}
	if bundle.Identity.DB2FileDataID == 0 {
		return TableReading{}, ErrContentMissing
	}
	rawManifest, err := r.store.ReadBlob(ctx, bundle.Manifest, 4<<20)
	if err != nil {
		return TableReading{}, err
	}
	manifest, err := schema.ParseManifest(ctx, rawManifest)
	if err != nil {
		return TableReading{}, err
	}
	identity, err := manifest.Lookup(ctx, bundle.Identity.Name)
	if err != nil {
		return TableReading{}, err
	}
	if identity != bundle.Identity || identity.DB2FileDataID == 0 || q.FileDataID != identity.DB2FileDataID {
		return TableReading{}, ErrDefinitionIdentity
	}
	rawDefinition, err := r.store.ReadBlob(ctx, bundle.Definition, 4<<20)
	if err != nil {
		return TableReading{}, err
	}
	doc, err := schema.Parse(ctx, rawDefinition)
	if err != nil {
		return TableReading{}, err
	}
	file, err := r.readFile(ctx, pin, q, true)
	if err != nil {
		return TableReading{}, err
	}
	content := file.Content
	if file.PartialContent != nil {
		content = *file.PartialContent
	}
	raw, err := r.store.ReadBlob(ctx, content, q.ContentBytes)
	if err != nil {
		return TableReading{}, err
	}
	budget := table.Budget{FileBytes: q.ContentBytes, MetadataBytes: 64 << 20, Rows: 1000000, Columns: 4096, Partitions: 4096}
	source := availableReader{ReaderAt: bytes.NewReader(raw), missing: file.Missing}
	layout, err := table.Inspect(ctx, source, int64(len(raw)), budget)
	if err != nil {
		return TableReading{}, err
	}
	if _, err := manifest.Resolve(ctx, identity.Name, q.FileDataID, layout.TableHash); err != nil {
		return TableReading{}, err
	}
	unavailable, skipped, err := unavailablePartitions(layout, file.Missing)
	if err != nil {
		return TableReading{}, err
	}
	rows, err := table.OpenAvailableRecords(ctx, source, int64(len(raw)), budget, skipped)
	if err != nil {
		return TableReading{}, err
	}
	view, err := rows.Bind(ctx, doc, pin.FullBuild)
	if err != nil {
		return TableReading{}, err
	}
	result := TableReading{File: file, Sources: bundle, LayoutHash: fmt.Sprintf("%08X", layout.LayoutHash), Schema: view.Definition()}
	result.Complete, result.UnavailablePartitions = file.ContentVerified, unavailable
	return consume(view, result)
}
