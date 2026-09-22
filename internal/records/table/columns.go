// SPDX-License-Identifier: AGPL-3.0-or-later
// WDC storage semantics adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package table

import (
	"context"
	"encoding/binary"
	"io"
)

// Columns compiles immutable per-column auxiliary storage. Values are raw bits;
// signed packed values are extended to 64 bits. Float/string/type interpretation
// requires a separately verified schema, never a guess from the values.
type Columns struct {
	layout   Layout
	programs []columnProgram
}

type columnProgram struct {
	storage Storage
	palette []uint32
	common  map[uint32]uint32
}

func OpenColumns(ctx context.Context, source io.ReaderAt, size int64, budget Budget) (*Columns, error) {
	layout, err := Inspect(ctx, source, size, budget)
	if err != nil {
		return nil, err
	}
	result := &Columns{layout: layout, programs: make([]columnProgram, len(layout.Fields))}
	paletteOffset, commonOffset := layout.PaletteOffset, layout.CommonOffset
	for i, s := range layout.Fields {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		program := columnProgram{storage: s}
		var offset int64
		switch s.Codec {
		case 2:
			offset = commonOffset
			commonOffset += int64(s.ExtraBytes)
		case 3, 4:
			offset = paletteOffset
			paletteOffset += int64(s.ExtraBytes)
		default:
			if s.ExtraBytes != 0 {
				return nil, ErrFormat
			}
			result.programs[i] = program
			continue
		}
		raw := make([]byte, int(s.ExtraBytes))
		n, err := source.ReadAt(raw, offset)
		if n != len(raw) {
			if err != nil {
				return nil, err
			}
			return nil, io.ErrUnexpectedEOF
		}
		if err != nil && err != io.EOF {
			return nil, err
		}
		if s.Codec == 2 {
			program.common = make(map[uint32]uint32)
			for pos := 0; pos < len(raw); pos += 8 {
				if pos&8191 == 0 {
					if err := ctx.Err(); err != nil {
						return nil, err
					}
				}
				id, value := binary.LittleEndian.Uint32(raw[pos:pos+4]), binary.LittleEndian.Uint32(raw[pos+4:pos+8])
				if _, exists := program.common[id]; exists {
					return nil, ErrFormat
				}
				program.common[id] = value
			}
		} else {
			program.palette = make([]uint32, len(raw)/4)
			for j := range program.palette {
				program.palette[j] = binary.LittleEndian.Uint32(raw[j*4 : j*4+4])
			}
		}
		result.programs[i] = program
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

// Values decodes one explicitly selected column from an isolated record slice.
// elements is the schema cardinality (1 for a scalar). No bytes outside the
// record are read. Corrupt indices never turn into default or zero values.
func (c *Columns) Values(ctx context.Context, record []byte, id uint32, column int, elements uint32) ([]uint64, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if c == nil || column < 0 || column >= len(c.programs) {
		return nil, ErrFormat
	}
	if elements == 0 || elements > 65536 {
		return nil, ErrLimit
	}
	p := c.programs[column]
	s := p.storage
	if s.Codec == 0 {
		if uint32(s.BitWidth)%elements != 0 {
			return nil, ErrFormat
		}
		width := uint32(s.BitWidth) / elements
		if width == 0 || width > 64 {
			return nil, ErrFormat
		}
		values := make([]uint64, int(elements))
		for i := range values {
			value, err := extractBits(record, uint64(s.BitOffset)+uint64(i)*uint64(width), width)
			if err != nil {
				return nil, err
			}
			values[i] = value
		}
		return values, nil
	}
	if s.Codec != 4 && elements != 1 {
		return nil, ErrFormat
	}
	if s.Codec == 2 {
		value := s.Parameters[0]
		if override, exists := p.common[id]; exists {
			value = override
		}
		return []uint64{uint64(value)}, nil
	}
	width := s.Parameters[1]
	value, err := extractBits(record, uint64(s.BitOffset), width)
	if err != nil {
		return nil, err
	}
	switch s.Codec {
	case 1:
		return []uint64{value}, nil
	case 5:
		if width > 0 && width < 64 {
			shift := 64 - width
			value = uint64(int64(value<<shift) >> shift)
		}
		return []uint64{value}, nil
	case 3:
		if value >= uint64(len(p.palette)) {
			return nil, ErrFormat
		}
		return []uint64{uint64(p.palette[value])}, nil
	case 4:
		if elements != s.Parameters[2] || value >= uint64(len(p.palette))/uint64(elements) {
			return nil, ErrFormat
		}
		values := make([]uint64, int(elements))
		base := value * uint64(elements)
		for i := range values {
			values[i] = uint64(p.palette[base+uint64(i)])
		}
		return values, nil
	default:
		return nil, ErrUnsupported
	}
}

func extractBits(record []byte, offset uint64, width uint32) (uint64, error) {
	if width > 64 || offset > uint64(len(record))*8 || uint64(width) > uint64(len(record))*8-offset {
		return 0, ErrFormat
	}
	var value uint64
	// Byte fragments also handle a non-aligned 64-bit field spanning nine bytes.
	for copied := uint32(0); copied < width; {
		position := offset + uint64(copied)
		shift := uint32(position & 7)
		count := min(uint32(8)-shift, width-copied)
		fragment := uint64(record[position/8]>>shift) & ((uint64(1) << count) - 1)
		value |= fragment << copied
		copied += count
	}
	return value, nil
}
