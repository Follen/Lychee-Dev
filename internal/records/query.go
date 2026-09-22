package records

import (
	"bytes"
	"context"
	"fmt"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/selection"
)

type TableReading struct {
	File       FileReading       `json:"file"`
	Sources    DefinitionBundle  `json:"sources"`
	LayoutHash string            `json:"layoutHash"`
	Schema     schema.Definition `json:"schema"`
	RecordID   *uint32           `json:"recordID,omitempty"`
	Row        map[string]any    `json:"row,omitempty"`
	Page       *table.RowPage    `json:"page,omitempty"`
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
	file, err := r.ReadFile(ctx, pin, q)
	if err != nil {
		return TableReading{}, err
	}
	raw, err := r.store.ReadBlob(ctx, file.Content, q.ContentBytes)
	if err != nil {
		return TableReading{}, err
	}
	budget := table.Budget{FileBytes: q.ContentBytes, MetadataBytes: 64 << 20, Rows: 1000000, Columns: 4096, Partitions: 4096}
	layout, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		return TableReading{}, err
	}
	if _, err := manifest.Resolve(ctx, identity.Name, q.FileDataID, layout.TableHash); err != nil {
		return TableReading{}, err
	}
	rows, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		return TableReading{}, err
	}
	view, err := rows.Bind(ctx, doc, pin.FullBuild)
	if err != nil {
		return TableReading{}, err
	}
	result := TableReading{File: file, Sources: bundle, LayoutHash: fmt.Sprintf("%08X", layout.LayoutHash), Schema: view.Definition()}
	return consume(view, result)
}
