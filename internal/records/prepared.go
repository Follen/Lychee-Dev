package records

import (
	"context"
	"fmt"

	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/selection"
)

// PrepareTable authenticates once and returns an immutable in-memory table
// plus its provenance. Repeated scans do not reopen CASC or resolve definitions.
func (r *Reader) PrepareTable(ctx context.Context, pin selection.DataPin, q FileQuery, bundle DefinitionBundle) (*table.View, TableReading, error) {
	var prepared *table.View
	reading, err := r.withTableView(ctx, pin, q, bundle, func(view *table.View, reading TableReading) (TableReading, error) {
		prepared = view
		return reading, nil
	})
	if err != nil {
		return nil, TableReading{}, err
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
	for _, field := range view.Definition().Fields {
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
	return relational.Source{Columns: columns, Scan: func(ctx context.Context, yield func([]any) error) error {
		return view.Scan(ctx, indexBytes, 1<<20, func(row map[string]any) error {
			values := make([]any, len(cells))
			for i, cell := range cells {
				value, ok := row[cell.name]
				if !ok {
					return table.ErrFormat
				}
				if cell.index >= 0 {
					array, ok := value.([]any)
					if !ok || cell.index >= len(array) {
						return table.ErrFormat
					}
					value = array[cell.index]
				}
				values[i] = value
			}
			return yield(values)
		})
	}}, nil
}
