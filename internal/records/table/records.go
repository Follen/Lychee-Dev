// SPDX-License-Identifier: AGPL-3.0-or-later
// WDC record layout semantics adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package table

import (
	"context"
	"encoding/binary"
	"errors"
	"io"
)

var ErrRecordMissing = errors.New("table.record_not_found")

// Records owns an immutable identity index, not a row cache. The caller must
// keep its fully decoded/content-verified source immutable and open throughout.
type Records struct {
	source    io.ReaderAt
	size      int64
	columns   *Columns
	locations map[uint32]recordLocation
}
type recordLocation struct {
	origin      uint32
	partition   int
	offset      int64
	length      uint32
	relation    uint32
	hasRelation bool
}

type Record struct {
	ID          uint32
	OriginID    uint32
	Partition   int
	Offset      int64
	Data        []byte
	Relation    uint32
	HasRelation bool
}

// OpenRecords indexes fixed and sparse WDC records, IDs, copy chains and
// relationships. Budget.Rows includes copies. Sparse typed decoding requires
// an ordered schema; Lookup returns the correctly bounded raw record for now.
func OpenRecords(ctx context.Context, source io.ReaderAt, size int64, budget Budget) (*Records, error) {
	columns, err := OpenColumns(ctx, source, size, budget)
	if err != nil {
		return nil, err
	}
	layout := columns.layout
	if layout.Stride > 1<<20 {
		return nil, ErrLimit
	}
	out := &Records{source: source, size: size, columns: columns, locations: make(map[uint32]recordLocation)}
	copies := make(map[uint32]uint32)
	var auxiliary int64 = layout.MetadataEnd
	for pi, p := range layout.Partitions {
		sparse := layout.Flags&1 != 0
		if !sparse && p.SparseCount != 0 {
			return nil, ErrUnsupported
		}
		if !sparse && p.IdentityBytes != 0 && uint64(p.IdentityBytes) != uint64(p.Rows)*4 {
			return nil, ErrFormat
		}
		auxBytes := int64(p.IdentityBytes) + int64(p.Copies)*8 + int64(p.RelationBytes)
		if sparse {
			if layout.Version == 2 {
				auxBytes += (int64(layout.LastID) - int64(layout.FirstID) + 1) * 6
			} else {
				auxBytes += int64(p.SparseCount) * 10
			}
		}
		if auxBytes > budget.MetadataBytes-auxiliary {
			return nil, ErrLimit
		}
		auxiliary += auxBytes
		position := int64(p.Offset) + int64(p.Rows)*int64(layout.Stride) + int64(p.StringBytes)
		if sparse {
			position = int64(p.SparseOffset)
		}
		aux, err := out.read(ctx, position, auxBytes)
		if err != nil {
			return nil, err
		}
		ids := make([]uint32, int(p.Rows))
		if sparse {
			ids, aux, err = out.indexSparse(ctx, pi, p, aux, budget.Rows)
			if err != nil {
				return nil, err
			}
		} else {
			row := make([]byte, int(layout.Stride))
			for ordinal := uint32(0); ordinal < p.Rows; ordinal++ {
				if err := ctx.Err(); err != nil {
					return nil, err
				}
				offset := int64(p.Offset) + int64(ordinal)*int64(layout.Stride)
				var id uint32
				if p.IdentityBytes > 0 {
					id = binary.LittleEndian.Uint32(aux[int(ordinal)*4 : int(ordinal)*4+4])
				} else {
					if int(layout.IdentityColumn) >= len(columns.programs) {
						return nil, ErrFormat
					}
					if columns.programs[layout.IdentityColumn].storage.Codec == 2 {
						return nil, ErrFormat
					}
					n, err := source.ReadAt(row, offset)
					if n != len(row) {
						if err != nil {
							return nil, err
						}
						return nil, io.ErrUnexpectedEOF
					}
					if err != nil && err != io.EOF {
						return nil, err
					}
					values, err := columns.Values(ctx, row, 0, int(layout.IdentityColumn), 1)
					if err != nil {
						return nil, err
					}
					if values[0] > 0xffffffff {
						return nil, ErrFormat
					}
					id = uint32(values[0])
				}
				if _, exists := out.locations[id]; exists {
					return nil, ErrFormat
				}
				out.locations[id] = recordLocation{origin: id, partition: pi, offset: offset, length: layout.Stride}
				ids[ordinal] = id
			}
		}
		cursor := int(p.IdentityBytes)
		for i := uint32(0); i < p.Copies; i++ {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			destination, origin := binary.LittleEndian.Uint32(aux[cursor:cursor+4]), binary.LittleEndian.Uint32(aux[cursor+4:cursor+8])
			cursor += 8
			if _, exists := copies[destination]; exists {
				return nil, ErrFormat
			}
			if uint64(layout.Rows)+uint64(len(copies))+1 > uint64(budget.Rows) {
				return nil, ErrLimit
			}
			copies[destination] = origin
		}
		if p.RelationBytes > 0 {
			if p.RelationBytes < 12 {
				return nil, ErrFormat
			}
			count := binary.LittleEndian.Uint32(aux[cursor : cursor+4])
			cursor += 12
			if uint64(count)*8+12 != uint64(p.RelationBytes) {
				return nil, ErrFormat
			}
			for i := uint32(0); i < count; i++ {
				if err := ctx.Err(); err != nil {
					return nil, err
				}
				foreign, ordinal := binary.LittleEndian.Uint32(aux[cursor:cursor+4]), binary.LittleEndian.Uint32(aux[cursor+4:cursor+8])
				cursor += 8
				id := ordinal
				if layout.Version < 4 || layout.Flags&2 == 0 {
					if ordinal >= uint32(len(ids)) {
						return nil, ErrFormat
					}
					id = ids[ordinal]
				}
				location := out.locations[id]
				if _, exists := out.locations[id]; !exists || location.partition != pi {
					return nil, ErrFormat
				}
				if location.hasRelation {
					return nil, ErrFormat
				}
				location.relation, location.hasRelation = foreign, true
				out.locations[id] = location
			}
		}
	}
	for id, location := range out.locations {
		if location.origin != id {
			out.locations[id] = out.locations[location.origin]
		}
	}
	for id := range copies {
		if _, exists := out.locations[id]; exists {
			return nil, ErrFormat
		}
	}
	if uint64(len(out.locations))+uint64(len(copies)) > uint64(budget.Rows) {
		return nil, ErrLimit
	}
	// Resolve and memoize each chain once; cycles and absent origins are errors.
	for destination := range copies {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		path := []uint32{}
		seen := make(map[uint32]bool)
		id := destination
		for {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			if location, exists := out.locations[id]; exists {
				for _, alias := range path {
					out.locations[alias] = location
				}
				break
			}
			if seen[id] {
				return nil, ErrFormat
			}
			seen[id] = true
			path = append(path, id)
			origin, exists := copies[id]
			if !exists {
				return nil, ErrFormat
			}
			id = origin
		}
	}
	return out, nil
}

func (r *Records) Lookup(ctx context.Context, id uint32) (Record, error) {
	if err := ctx.Err(); err != nil {
		return Record{}, err
	}
	location, ok := r.locations[id]
	if !ok {
		return Record{}, ErrRecordMissing
	}
	data, err := r.read(ctx, location.offset, int64(location.length))
	if err != nil {
		return Record{}, err
	}
	return Record{ID: id, OriginID: location.origin, Partition: location.partition, Offset: location.offset, Data: data, Relation: location.relation, HasRelation: location.hasRelation}, nil
}

func (r *Records) Values(ctx context.Context, id uint32, column int, elements uint32) ([]uint64, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if r.columns.layout.Flags&1 != 0 {
		return nil, ErrUnsupported
	}
	row, err := r.Lookup(ctx, id)
	if err != nil {
		return nil, err
	}
	if r.columns.layout.Partitions[row.Partition].IdentityBytes == 0 && column == int(r.columns.layout.IdentityColumn) {
		if elements != 1 {
			return nil, ErrFormat
		}
		return []uint64{uint64(id)}, nil
	}
	return r.columns.Values(ctx, row.Data, row.OriginID, column, elements)
}

func (r *Records) read(ctx context.Context, offset, length int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if offset < 0 || length < 0 || offset > r.size || length > r.size-offset {
		return nil, ErrFormat
	}
	data := make([]byte, int(length))
	n, err := r.source.ReadAt(data, offset)
	if n != len(data) {
		if err != nil {
			return nil, err
		}
		return nil, io.ErrUnexpectedEOF
	}
	if err != nil && err != io.EOF {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}
