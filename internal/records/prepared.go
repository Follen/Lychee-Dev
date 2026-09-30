package records

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"

	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/selection"
)

type preparedSessionTable struct {
	view    *table.View
	reading TableReading
}

// PrepareTable authenticates once and returns an immutable in-memory table
// plus its provenance. Repeated scans do not reopen CASC or resolve definitions.
func (r *Reader) PrepareTable(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle) (*table.View, TableReading, error) {
	encoded, err := json.Marshal(bundle)
	if err != nil {
		return nil, TableReading{}, err
	}
	key := string(encoded)
	if r.session != nil {
		if cached, ok := r.session.views[key]; ok {
			if err := r.session.prepare(ctx, r, pin, q); err != nil {
				return nil, TableReading{}, err
			}
			content := cached.reading.File.Content
			if cached.reading.File.PartialContent != nil {
				content = *cached.reading.File.PartialContent
			}
			if q.FileDataID != bundle.Identity.DB2FileDataID || content.Bytes > q.ContentBytes {
				return nil, TableReading{}, ErrMetadataLimit
			}
			return cached.view, cached.reading, nil
		}
		if len(r.session.views) >= 32 {
			return nil, TableReading{}, table.ErrLimit
		}
	}
	var prepared *table.View
	reading, err := r.withTableView(ctx, pin, q, bundle, func(view *table.View, reading TableReading) (TableReading, error) {
		prepared = view
		return reading, nil
	})
	if err != nil {
		return nil, TableReading{}, err
	}
	if r.session != nil {
		r.session.views[key] = preparedSessionTable{prepared, reading}
	}
	return prepared, reading, nil
}

// TableSource exposes DB2 arrays as scalar columns Name[0], Name[1], ...;
// quote these identifiers in SQL. Original named arrays remain in data db2.
func TableSource(view *table.View, indexBytes int64) (relational.Source, error) {
	if view == nil || indexBytes < 0 {
		return relational.Source{}, table.ErrLimit
	}
	type cell struct {
		name  string
		index int
	}
	var cells []cell
	var columns []string
	identity := ""
	var count uint64
	for _, field := range view.Definition().Fields {
		if field.Array {
			count += uint64(field.Elements)
		} else {
			count++
		}
		if count > 4096 {
			return relational.Source{}, table.ErrLimit
		}
	}
	if err := view.ReserveMetadata(int64(count) * 128); err != nil {
		return relational.Source{}, err
	}
	cells = make([]cell, 0, int(count))
	columns = make([]string, 0, int(count))
	for _, field := range view.Definition().Fields {
		if field.Identity {
			identity = field.Name
		}
		if field.Array {
			for i := uint32(0); i < field.Elements; i++ {
				columns = append(columns, fmt.Sprintf("%s[%d]", field.Name, i))
				cells = append(cells, cell{field.Name, int(i)})
			}
		} else {
			columns = append(columns, field.Name)
			cells = append(cells, cell{field.Name, -1})
		}
		if len(columns) > 4096 {
			return relational.Source{}, table.ErrLimit
		}
	}
	flatten := func(row map[string]any, selected map[int]bool, yield func([]any) error) error {
		values := make([]any, len(cells))
		for i, cell := range cells {
			if selected != nil && !selected[i] {
				continue
			}
			value, ok := row[cell.name]
			if !ok {
				return table.ErrFormat
			}
			if cell.index >= 0 {
				switch array := value.(type) {
				case []any:
					if cell.index >= len(array) {
						return table.ErrFormat
					}
					value = array[cell.index]
				case []string:
					if cell.index >= len(array) {
						return table.ErrFormat
					}
					value = array[cell.index]
				default:
					return table.ErrFormat
				}
			}
			values[i] = value
		}
		return yield(values)
	}
	return relational.Source{Columns: columns, Scan: func(ctx context.Context, yield func([]any) error) error {
		return view.Scan(ctx, indexBytes, 1<<20, func(row map[string]any) error { return flatten(row, nil, yield) })
	}, ScanSelected: func(ctx context.Context, indices []int, yield func([]any) error) error {
		selected := map[int]bool{}
		names := []string{}
		for _, i := range indices {
			if i < 0 || i >= len(cells) {
				return relational.ErrBinding
			}
			selected[i] = true
			names = append(names, cells[i].name)
		}
		projected, err := view.WithProjection(names)
		if err != nil {
			return err
		}
		return projected.Scan(ctx, indexBytes, 1<<20, func(row map[string]any) error { return flatten(row, selected, yield) })
	}, Equal: func(ctx context.Context, column string, value any, yield func([]any) error) (bool, error) {
		if !strings.EqualFold(column, identity) {
			return false, nil
		}
		var id uint32
		switch v := value.(type) {
		case nil:
			return true, nil
		case int64:
			if v < 0 || v > math.MaxUint32 {
				return true, nil
			}
			id = uint32(v)
		case uint64:
			if v > math.MaxUint32 {
				return true, nil
			}
			id = uint32(v)
		case float64:
			if math.IsNaN(v) || math.IsInf(v, 0) {
				return true, relational.ErrNumericRange
			}
			if v < 0 || v > math.MaxUint32 || v != math.Trunc(v) {
				return true, nil
			}
			id = uint32(v)
		default:
			return true, relational.ErrType
		}
		row, err := view.Row(ctx, id, 1<<20)
		if errors.Is(err, table.ErrRecordMissing) {
			return true, nil
		}
		if err != nil {
			return true, err
		}
		return true, flatten(row, nil, yield)
	}}, nil
}
