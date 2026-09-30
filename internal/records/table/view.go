package table

import (
	"bytes"
	"context"
	"fmt"
	"math"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/records/schema"
)

type View struct {
	records    *Records
	definition schema.Definition
	columns    []int
	selected   map[string]bool
}

// ReserveMetadata includes adapter-owned maps/slices in the prepared view's
// query budget. Adapters call this before allocating their own structures.
func (v *View) ReserveMetadata(bytes int64) error {
	if v == nil || v.records == nil {
		return ErrFormat
	}
	return v.records.budget.Charge(resource.Cost{RetainedBytes: bytes, MetadataBytes: bytes})
}

// WithProjection reduces materialized fields while retaining validation of all
// fields and sparse padding. It never turns a corrupt unused field into success.
func (v *View) WithProjection(names []string) (*View, error) {
	if err := v.ReserveMetadata(int64(len(names)) * 128); err != nil {
		return nil, err
	}
	selected := map[string]bool{}
	for _, name := range names {
		found := false
		for _, f := range v.definition.Fields {
			found = found || f.Name == name
		}
		if !found {
			return nil, ErrFormat
		}
		selected[name] = true
	}
	copy := *v
	copy.selected = selected
	return &copy, nil
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
	if err := r.budget.Charge(resource.Cost{RetainedBytes: int64(len(definition.Fields)) * 192, MetadataBytes: int64(len(definition.Fields)) * 192}); err != nil {
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
	var work int64 = 1
	for _, f := range v.definition.Fields {
		work += int64(f.Elements)
	}
	if err := v.records.budget.Charge(resource.Cost{DecodeWork: work}); err != nil {
		return nil, err
	}
	row, err := v.records.Lookup(ctx, id)
	if err != nil {
		return nil, err
	}
	capacity := len(v.definition.Fields)
	if v.selected != nil {
		capacity = len(v.selected)
	}
	result := make(map[string]any, capacity)
	sparse := v.records.columns.layout.Flags&1 != 0
	cursor := uint64(0)
	remaining := textBytes
	for index, field := range v.definition.Fields {
		wanted := v.selected == nil || v.selected[field.Name]
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if !field.Inline {
			if !wanted {
				continue
			}
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
			if !wanted {
				continue
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
			if wanted {
				result[field.Name] = uint64(id)
			}
			continue
		}
		var converted []any
		if wanted {
			converted = make([]any, len(values))
		}
		for i, value := range values {
			number, convertErr := typedNumber(value, field)
			err = convertErr
			if err != nil {
				return nil, err
			}
			if wanted {
				converted[i] = number
			}
		}
		if !wanted {
			continue
		}
		if field.Array {
			result[field.Name] = converted
		} else {
			result[field.Name] = converted[0]
		}
	}
	if sparse {
		total := uint64(len(row.Data)) * 8
		// Sparse records may be padded to a 32-bit boundary (observed in the
		// pinned retail WDC5 Spell table). Keep accepting packed-byte padding,
		// but reject excess/alignment gaps and all nonzero trailing bits.
		paddingLimit := uint64(7)
		if total%32 == 0 {
			paddingLimit = 31
		}
		if cursor > total || total-cursor > paddingLimit {
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
