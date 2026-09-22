package records

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"math"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

var ErrCacheFields = errors.New("records.hotfix_fields")

// DecodeHotfixFields reads the sequential XFTH payload, not WDC bitpacking.
// Strings are zero-terminated UTF-8, non-inline IDs come from the record header,
// and non-inline relations are present in the payload at their declared width.
// The selected definition must describe the exact cache build; there is no
// nearest-build or partial-decoding fallback.
func DecodeHotfixFields(ctx context.Context, raw []byte, recordID uint32, definition schema.Definition) (map[string]any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(raw) > 8<<20 || len(definition.Fields) > 4096 {
		return nil, ErrCacheLimit
	}
	if len(definition.Fields) == 0 {
		return nil, ErrCacheFields
	}
	seen := make(map[string]bool, len(definition.Fields))
	var cells uint64
	identities := 0
	for _, f := range definition.Fields {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if f.Name == "" || seen[f.Name] || f.Elements == 0 || !f.Array && f.Elements != 1 {
			return nil, ErrCacheFields
		}
		seen[f.Name] = true
		cells += uint64(f.Elements)
		if cells > 65536 {
			return nil, ErrCacheLimit
		}
		switch f.Kind {
		case "int":
			if f.Bits != 8 && f.Bits != 16 && f.Bits != 32 && f.Bits != 64 {
				return nil, ErrCacheFields
			}
		case "float":
			if f.Bits != 32 {
				return nil, ErrCacheFields
			}
		case "string", "locstring":
		default:
			return nil, ErrCacheFields
		}
		if f.Identity {
			identities++
			if identities > 1 || f.Array || f.Kind != "int" {
				return nil, ErrCacheFields
			}
		}
		if !f.Inline && (f.Kind != "int" || f.Array || !f.Identity && !f.Relation) {
			return nil, ErrCacheFields
		}
	}
	result := make(map[string]any, len(definition.Fields))
	offset, textRemaining := 0, 1<<20
	for _, f := range definition.Fields {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if f.Identity && !f.Inline {
			if f.Signed {
				n := int64(int32(recordID))
				if f.Bits < 64 && (n < -(int64(1)<<(f.Bits-1)) || n > (int64(1)<<(f.Bits-1))-1) {
					return nil, ErrCacheFields
				}
				result[f.Name] = n
			} else {
				n := uint64(recordID)
				if f.Bits < 64 && n >= uint64(1)<<f.Bits {
					return nil, ErrCacheFields
				}
				result[f.Name] = n
			}
			continue
		}
		values := make([]any, 0, f.Elements)
		for i := uint32(0); i < f.Elements; i++ {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			var value any
			if f.Kind == "string" || f.Kind == "locstring" {
				available := raw[offset:]
				end := bytes.IndexByte(available[:min(len(available), textRemaining+1)], 0)
				if end < 0 {
					if len(available) > textRemaining {
						return nil, ErrCacheLimit
					}
					return nil, fmt.Errorf("%w: unterminated %s", ErrCacheFields, f.Name)
				}
				if !utf8.Valid(available[:end]) {
					return nil, fmt.Errorf("%w: invalid UTF-8 in %s", ErrCacheFields, f.Name)
				}
				value = string(available[:end])
				offset += end + 1
				textRemaining -= end
			} else {
				width := int(f.Bits / 8)
				if len(raw)-offset < width {
					return nil, fmt.Errorf("%w: truncated %s", ErrCacheFields, f.Name)
				}
				part := raw[offset : offset+width]
				var n uint64
				for j, b := range part {
					n |= uint64(b) << (8 * j)
				}
				offset += width
				if f.Kind == "float" {
					number := float64(math.Float32frombits(binary.LittleEndian.Uint32(part)))
					if math.IsInf(number, 0) || math.IsNaN(number) {
						return nil, fmt.Errorf("%w: non-finite %s", ErrCacheFields, f.Name)
					}
					value = number
				} else if f.Signed {
					shift := 64 - f.Bits
					value = int64(n<<shift) >> shift
				} else {
					value = n
				}
			}
			if f.Identity {
				if f.Signed && value != int64(int32(recordID)) || !f.Signed && value != uint64(recordID) {
					return nil, fmt.Errorf("%w: inline ID disagrees with header", ErrCacheFields)
				}
			}
			values = append(values, value)
		}
		if f.Array {
			result[f.Name] = values
		} else {
			result[f.Name] = values[0]
		}
	}
	if offset != len(raw) {
		return nil, fmt.Errorf("%w: %d unconsumed payload bytes", ErrCacheFields, len(raw)-offset)
	}
	return result, nil
}
