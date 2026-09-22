// SPDX-License-Identifier: AGPL-3.0-or-later
// WDC layouts adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package table

import (
	"context"
	"encoding/binary"
	"errors"
	"io"
)

var (
	ErrFormat      = errors.New("table.invalid_format")
	ErrLimit       = errors.New("table.limit_exceeded")
	ErrUnsupported = errors.New("table.unsupported_format")
)

type Budget struct {
	FileBytes     int64
	MetadataBytes int64
	Rows          uint32
	Columns       uint32
	Partitions    uint32
}

// Layout is structural metadata, not a decoded table or compatibility verdict.
// Hashes retain the database's numeric identities for exact DBD binding.
type Layout struct {
	Version        int
	SchemaRevision uint32
	SchemaBuild    [128]byte
	Rows           uint32
	Columns        uint32
	Stride         uint32
	TableHash      uint32
	LayoutHash     uint32
	FirstID        uint32
	LastID         uint32
	Locale         uint32
	Flags          uint16
	IdentityColumn uint16
	PackedOffset   uint32
	LookupColumns  uint32
	Fields         []Storage
	Placements     []Placement
	Partitions     []Partition
	PaletteOffset  int64
	PaletteBytes   uint32
	CommonOffset   int64
	CommonBytes    uint32
	MetadataEnd    int64
}

type Placement struct {
	WidthDelta int16
	ByteOffset uint16
}

type Storage struct {
	BitOffset  uint16
	BitWidth   uint16
	ExtraBytes uint32
	Codec      uint32
	Parameters [3]uint32
}

type Partition struct {
	KeyID         uint64
	Offset        uint32
	Rows          uint32
	StringBytes   uint32
	IdentityBytes uint32
	RelationBytes uint32
	Copies        uint32
	SparseOffset  uint32
	SparseCount   uint32
}

// Inspect reads only bounded WDC metadata from an immutable, already verified
// content source. It does not infer missing decryption keys, execute schemas,
// materialize rows or read the complete file into memory.
func Inspect(ctx context.Context, source io.ReaderAt, size int64, budget Budget) (Layout, error) {
	var out Layout
	if err := ctx.Err(); err != nil {
		return out, err
	}
	if source == nil || size < 4 {
		return out, ErrFormat
	}
	if budget.FileBytes <= 0 || budget.FileBytes > 1<<40 || size > budget.FileBytes || budget.MetadataBytes <= 0 || budget.MetadataBytes > 64<<20 || budget.Rows == 0 || budget.Columns == 0 || budget.Columns > 65536 || budget.Partitions == 0 || budget.Partitions > 65536 {
		return out, ErrLimit
	}
	r := metadataReader{ctx: ctx, source: source, size: size, budget: budget.MetadataBytes}
	magic, err := r.take(4)
	if err != nil {
		return Layout{}, err
	}
	switch string(magic) {
	case "WDC2", "1SLC":
		out.Version = 2
	case "WDC3":
		out.Version = 3
	case "WDC4":
		out.Version = 4
	case "WDC5":
		out.Version = 5
	default:
		return Layout{}, ErrUnsupported
	}
	if out.Version == 5 {
		b, err := r.take(132)
		if err != nil {
			return Layout{}, err
		}
		out.SchemaRevision = binary.LittleEndian.Uint32(b[:4])
		copy(out.SchemaBuild[:], b[4:])
	}
	header, err := r.take(68)
	if err != nil {
		return Layout{}, err
	}
	u32 := func(offset int) uint32 { return binary.LittleEndian.Uint32(header[offset : offset+4]) }
	out.Rows, out.Columns, out.Stride = u32(0), u32(4), u32(8)
	out.TableHash, out.LayoutHash = u32(16), u32(20)
	out.FirstID, out.LastID, out.Locale = u32(24), u32(28), u32(32)
	out.Flags = binary.LittleEndian.Uint16(header[36:38])
	out.IdentityColumn = binary.LittleEndian.Uint16(header[38:40])
	totalColumns := u32(40)
	out.PackedOffset, out.LookupColumns = u32(44), u32(48)
	storageBytes := u32(52)
	out.CommonBytes, out.PaletteBytes = u32(56), u32(60)
	partitionCount := u32(64)
	if out.Rows > budget.Rows || totalColumns > budget.Columns || out.Columns > budget.Columns || partitionCount > budget.Partitions {
		return Layout{}, ErrLimit
	}
	if out.Rows > 0 && (partitionCount == 0 || (out.Stride == 0 && out.Flags&1 == 0) || out.LastID < out.FirstID) {
		return Layout{}, ErrFormat
	}
	if storageBytes%24 != 0 || storageBytes/24 > totalColumns {
		return Layout{}, ErrFormat
	}
	out.Partitions = make([]Partition, 0, partitionCount)
	var partitionRows uint64
	for i := uint32(0); i < partitionCount; i++ {
		length := int64(40)
		if out.Version == 2 {
			length = 36
		}
		b, err := r.take(length)
		if err != nil {
			return Layout{}, err
		}
		word := func(offset int) uint32 { return binary.LittleEndian.Uint32(b[offset : offset+4]) }
		p := Partition{KeyID: binary.LittleEndian.Uint64(b[:8]), Offset: word(8), Rows: word(12), StringBytes: word(16)}
		if out.Version == 2 {
			if word(20)%8 != 0 {
				return Layout{}, ErrFormat
			}
			p.Copies, p.SparseOffset, p.IdentityBytes, p.RelationBytes = word(20)/8, word(24), word(28), word(32)
		} else {
			p.SparseOffset, p.IdentityBytes, p.RelationBytes, p.SparseCount, p.Copies = word(20), word(24), word(28), word(32), word(36)
		}
		if p.IdentityBytes%4 != 0 {
			return Layout{}, ErrFormat
		}
		partitionRows += uint64(p.Rows)
		if partitionRows > uint64(budget.Rows) {
			return Layout{}, ErrLimit
		}
		out.Partitions = append(out.Partitions, p)
	}
	if partitionRows != uint64(out.Rows) {
		return Layout{}, ErrFormat
	}
	placements, err := r.take(int64(totalColumns) * 4)
	if err != nil {
		return Layout{}, err
	}
	for i := uint32(0); i < totalColumns; i++ {
		out.Placements = append(out.Placements, Placement{WidthDelta: int16(binary.LittleEndian.Uint16(placements[i*4 : i*4+2])), ByteOffset: binary.LittleEndian.Uint16(placements[i*4+2 : i*4+4])})
	}
	for i := uint32(0); i < storageBytes/24; i++ {
		b, err := r.take(24)
		if err != nil {
			return Layout{}, err
		}
		s := Storage{BitOffset: binary.LittleEndian.Uint16(b[:2]), BitWidth: binary.LittleEndian.Uint16(b[2:4]), ExtraBytes: binary.LittleEndian.Uint32(b[4:8]), Codec: binary.LittleEndian.Uint32(b[8:12])}
		for j := range s.Parameters {
			s.Parameters[j] = binary.LittleEndian.Uint32(b[12+j*4 : 16+j*4])
		}
		if s.Codec > 5 {
			return Layout{}, ErrUnsupported
		}
		if s.Codec == 2 && s.ExtraBytes%8 != 0 {
			return Layout{}, ErrFormat
		}
		if (s.Codec == 3 || s.Codec == 4) && s.ExtraBytes%4 != 0 {
			return Layout{}, ErrFormat
		}
		if s.Codec == 4 && (s.Parameters[2] == 0 || s.ExtraBytes/4%s.Parameters[2] != 0) {
			return Layout{}, ErrFormat
		}
		out.Fields = append(out.Fields, s)
	}
	var palette, common uint64
	for _, s := range out.Fields {
		if s.Codec == 2 {
			common += uint64(s.ExtraBytes)
		}
		if s.Codec == 3 || s.Codec == 4 {
			palette += uint64(s.ExtraBytes)
		}
	}
	if palette != uint64(out.PaletteBytes) || common != uint64(out.CommonBytes) {
		return Layout{}, ErrFormat
	}
	out.PaletteOffset = r.offset
	if err := r.skip(int64(out.PaletteBytes)); err != nil {
		return Layout{}, err
	}
	out.CommonOffset = r.offset
	if err := r.skip(int64(out.CommonBytes)); err != nil {
		return Layout{}, err
	}
	// WDC4+ stores an encrypted-ID list for each partition with a key identity.
	if out.Version >= 4 {
		for _, partition := range out.Partitions {
			if partition.KeyID == 0 {
				continue
			}
			b, err := r.take(4)
			if err != nil {
				return Layout{}, err
			}
			if err := r.skip(int64(binary.LittleEndian.Uint32(b)) * 4); err != nil {
				return Layout{}, err
			}
		}
	}
	out.MetadataEnd = r.offset
	var previousEnd int64 = out.MetadataEnd
	for _, p := range out.Partitions {
		start := int64(p.Offset)
		if start < previousEnd || start > size {
			return Layout{}, ErrFormat
		}
		// Multiply unsigned first: two uint32 operands can overflow int64.
		if out.Flags&1 == 0 && uint64(p.Rows)*uint64(out.Stride) > uint64(size-start) {
			return Layout{}, ErrFormat
		}
		payload := int64(p.Rows)*int64(out.Stride) + int64(p.StringBytes)
		if out.Flags&1 != 0 {
			if int64(p.SparseOffset) < start {
				return Layout{}, ErrFormat
			}
			payload = int64(p.SparseOffset) - start
			if out.Version == 2 {
				payload += (int64(out.LastID) - int64(out.FirstID) + 1) * 6
			}
		}
		payload += int64(p.IdentityBytes) + int64(p.RelationBytes) + int64(p.Copies)*8
		if out.Version >= 3 {
			payload += int64(p.SparseCount) * 10
		}
		if payload < 0 || payload > size-start {
			return Layout{}, ErrFormat
		}
		previousEnd = start + payload
	}
	if err := ctx.Err(); err != nil {
		return Layout{}, err
	}
	return out, nil
}

type metadataReader struct {
	ctx                  context.Context
	source               io.ReaderAt
	size, budget, offset int64
}

func (r *metadataReader) skip(n int64) error {
	if err := r.ctx.Err(); err != nil {
		return err
	}
	if n < 0 || n > r.size-r.offset {
		return ErrFormat
	}
	if n > r.budget-r.offset {
		return ErrLimit
	}
	r.offset += n
	return nil
}
func (r *metadataReader) take(n int64) ([]byte, error) {
	start := r.offset
	if err := r.skip(n); err != nil {
		return nil, err
	}
	b := make([]byte, int(n))
	count, err := r.source.ReadAt(b, start)
	if count != len(b) {
		if err != nil {
			return nil, err
		}
		return nil, io.ErrUnexpectedEOF
	}
	if err != nil && err != io.EOF {
		return nil, err
	}
	return b, nil
}
