package table

import (
	"bytes"
	"context"
	"fmt"
	"math"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

type View struct {
	records    *Records
	definition schema.Definition
	columns    []int
}

// Bind selects by the actual WDC layout hash and verifies physical column/ID
// placement. The source definition remains immutable and its identity survives
// binding. It never falls back to an unrelated build definition.
func (r *Records) Bind(ctx context.Context, doc *schema.Document, build string) (*View, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if doc == nil {
		return nil, ErrFormat
	}
	definition, err := doc.Select(ctx, build, fmt.Sprintf("%08X", r.columns.layout.LayoutHash))
	if err != nil {
		return nil, err
	}
	view := &View{records: r, definition: definition}
	inline := 0
	identities := 0
	var cells uint64
	for _, field := range definition.Fields {
		cells += uint64(field.Elements)
		if cells > 65536 {
			return nil, ErrLimit
		}
		column := -1
		if field.Inline {
			column = inline
			inline++
			if column >= len(r.columns.programs) {
				return nil, ErrFormat
			}
			storage := r.columns.programs[column].storage
			if r.columns.layout.Flags&1 == 0 && storage.Codec == 0 {
				width := uint32(field.Bits)
				if field.Kind == "string" || field.Kind == "locstring" {
					width = 32
					if storage.BitOffset%8 != 0 {
						return nil, ErrFormat
					}
				}
				if width*field.Elements != uint32(storage.BitWidth) {
					return nil, ErrFormat
				}
			}
			if storage.Codec == 4 && storage.Parameters[2] != field.Elements {
				return nil, ErrFormat
			}
		}
		if field.Identity {
			identities++
			if field.Array {
				return nil, ErrFormat
			}
			if r.columns.layout.Flags&1 == 0 {
				for _, p := range r.columns.layout.Partitions {
					if p.Rows > 0 && field.Inline != (p.IdentityBytes == 0) {
						return nil, ErrFormat
					}
				}
				if field.Inline && column != int(r.columns.layout.IdentityColumn) {
					return nil, ErrFormat
				}
			}
		}
		view.columns = append(view.columns, column)
	}
	if identities != 1 || inline != len(r.columns.programs) {
		return nil, ErrFormat
	}
	return view, nil
}

func (v *View) Definition() schema.Definition {
	d := v.definition
	d.Fields = append([]schema.Field(nil), d.Fields...)
	return d
}

// Row decodes one row with a shared UTF-8 output budget. Integer outputs retain
// their 64-bit Go types; adapters must not round them through floating point.
// Missing relationships are nil, distinct from a relationship whose ID is zero.
func (v *View) Row(ctx context.Context, id uint32, textBytes int) (map[string]any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if textBytes <= 0 || textBytes > 1<<20 {
		return nil, ErrLimit
	}
	row, err := v.records.Lookup(ctx, id)
	if err != nil {
		return nil, err
	}
	result := make(map[string]any, len(v.definition.Fields))
	sparse := v.records.columns.layout.Flags&1 != 0
	cursor := uint64(0)
	remaining := textBytes
	for index, field := range v.definition.Fields {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if !field.Inline {
			if field.Identity {
				result[field.Name] = uint64(id)
			} else if row.HasRelation {
				result[field.Name] = uint64(row.Relation)
			} else {
				result[field.Name] = nil
			}
			continue
		}
		column := v.columns[index]
		if field.Kind == "string" || field.Kind == "locstring" {
			var strings []string
			if sparse {
				if cursor%8 != 0 {
					return nil, ErrFormat
				}
				for i := uint32(0); i < field.Elements; i++ {
					if cursor/8 >= uint64(len(row.Data)) {
						return nil, ErrFormat
					}
					start := cursor / 8
					available := row.Data[start:]
					limit := min(len(available), remaining+1)
					zero := bytes.IndexByte(available[:limit], 0)
					if zero < 0 {
						if len(available) > remaining {
							return nil, ErrLimit
						}
						return nil, ErrFormat
					}
					value := available[:zero]
					if !utf8.Valid(value) {
						return nil, ErrFormat
					}
					strings = append(strings, string(value))
					remaining -= len(value)
					cursor += uint64(zero+1) * 8
				}
			} else {
				// A zero budget may still admit an empty string.
				storage := v.records.columns.programs[column].storage
				if storage.Codec != 0 {
					return nil, ErrUnsupported
				}
				strings, err = v.records.recordStrings(ctx, row, storage, field.Elements, max(1, remaining))
				if err != nil {
					return nil, err
				}
				for _, value := range strings {
					remaining -= len(value)
				}
				if remaining < 0 {
					return nil, ErrLimit
				}
			}
			if field.Array {
				result[field.Name] = strings
			} else {
				result[field.Name] = strings[0]
			}
			continue
		}
		var values []uint64
		if sparse {
			program := v.records.columns.programs[column]
			if program.storage.Codec == 0 {
				for i := uint32(0); i < field.Elements; i++ {
					value, err := extractBits(row.Data, cursor, uint32(field.Bits))
					if err != nil {
						return nil, err
					}
					values = append(values, value)
					cursor += uint64(field.Bits)
				}
			} else {
				if cursor/8 > uint64(len(row.Data)) {
					return nil, ErrFormat
				}
				program.storage.BitOffset = uint16(cursor % 8)
				decoder := Columns{programs: []columnProgram{program}}
				values, err = decoder.Values(ctx, row.Data[cursor/8:], row.OriginID, 0, field.Elements)
				if err != nil {
					return nil, err
				}
				if program.storage.Codec != 2 {
					cursor += uint64(program.storage.Parameters[1])
				}
			}
		} else {
			values, err = v.records.columns.Values(ctx, row.Data, row.OriginID, column, field.Elements)
			if err != nil {
				return nil, err
			}
		}
		if field.Identity {
			if len(values) != 1 || values[0] != uint64(row.OriginID) {
				return nil, ErrFormat
			}
			result[field.Name] = uint64(id)
			continue
		}
		converted := make([]any, len(values))
		for i, value := range values {
			converted[i], err = typedNumber(value, field)
			if err != nil {
				return nil, err
			}
		}
		if field.Array {
			result[field.Name] = converted
		} else {
			result[field.Name] = converted[0]
		}
	}
	if sparse {
		total := uint64(len(row.Data)) * 8
		if cursor > total || total-cursor > 7 {
			return nil, ErrFormat
		}
		padding, err := extractBits(row.Data, cursor, uint32(total-cursor))
		if err != nil || padding != 0 {
			return nil, ErrFormat
		}
	}
	return result, nil
}

func typedNumber(value uint64, field schema.Field) (any, error) {
	if field.Kind == "float" {
		number := math.Float32frombits(uint32(value))
		if math.IsNaN(float64(number)) || math.IsInf(float64(number), 0) {
			return nil, ErrFormat
		}
		return number, nil
	}
	if field.Kind != "int" || field.Bits == 0 || field.Bits > 64 {
		return nil, ErrFormat
	}
	if field.Signed {
		shift := 64 - field.Bits
		return int64(value<<shift) >> shift, nil
	}
	if field.Bits < 64 {
		value &= (uint64(1) << field.Bits) - 1
	}
	return value, nil
}
