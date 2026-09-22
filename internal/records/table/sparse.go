package table

import (
	"context"
	"encoding/binary"
	"sort"
)

// indexSparse normalizes auxiliary ordering for the shared copy/relation reader.
// It never treats gaps or zero map entries as materialized zero-filled records.
func (r *Records) indexSparse(ctx context.Context, partition int, p Partition, aux []byte, maxRows uint32) ([]uint32, []byte, error) {
	layout := r.columns.layout
	if p.StringBytes != 0 {
		return nil, nil, ErrFormat
	}
	var count int64
	mapStart, idStart, relationStart, copyStart := int64(0), int64(0), int64(0), int64(0)
	if layout.Version == 2 {
		count = int64(layout.LastID) - int64(layout.FirstID) + 1
		copyStart = count*6 + int64(p.IdentityBytes)
		relationStart = copyStart + int64(p.Copies)*8
	} else {
		count = int64(p.SparseCount)
		if uint64(count) != uint64(p.Rows) {
			return nil, nil, ErrFormat
		}
		copyStart = int64(p.IdentityBytes)
		mapStart = copyStart + int64(p.Copies)*8
		relationStart = mapStart + count*6
		idStart = relationStart + int64(p.RelationBytes)
		if layout.Version >= 4 && layout.Flags&2 != 0 {
			idStart = relationStart
			relationStart += count * 4
		}
	}
	if count < 0 || mapStart+count*6 > int64(len(aux)) || relationStart+int64(p.RelationBytes) > int64(len(aux)) {
		return nil, nil, ErrFormat
	}
	if int64(p.Rows) > count {
		return nil, nil, ErrFormat
	}
	ids := make([]uint32, 0, p.Rows)
	type extent struct{ start, end int64 }
	intervals := make([]extent, 0, p.Rows)
	byOffset := make(map[int64]recordLocation)
	for i := int64(0); i < count; i++ {
		if err := ctx.Err(); err != nil {
			return nil, nil, err
		}
		entry := aux[mapStart+i*6 : mapStart+i*6+6]
		offset, length := int64(binary.LittleEndian.Uint32(entry[:4])), uint32(binary.LittleEndian.Uint16(entry[4:6]))
		if layout.Version == 2 && offset == 0 && length == 0 {
			continue
		}
		if length == 0 || offset < int64(p.Offset) || offset > int64(p.SparseOffset) || int64(length) > int64(p.SparseOffset)-offset {
			return nil, nil, ErrFormat
		}
		id := layout.FirstID + uint32(i)
		if layout.Version >= 3 {
			if idStart+count*4 > int64(len(aux)) {
				return nil, nil, ErrFormat
			}
			id = binary.LittleEndian.Uint32(aux[idStart+i*4 : idStart+i*4+4])
		}
		if _, exists := r.locations[id]; exists {
			return nil, nil, ErrFormat
		}
		if uint64(len(r.locations))+1 > uint64(maxRows) {
			return nil, nil, ErrLimit
		}
		location := recordLocation{origin: id, partition: partition, offset: offset, length: length}
		previous, alias := byOffset[offset]
		if alias {
			if previous.length != length {
				return nil, nil, ErrFormat
			}
			if layout.Version == 2 {
				location.origin = previous.origin
			}
		} else {
			byOffset[offset] = location
			intervals = append(intervals, extent{offset, offset + int64(length)})
		}
		r.locations[id] = location
		if !alias || layout.Version >= 3 {
			ids = append(ids, id)
		}
	}
	if len(ids) != int(p.Rows) {
		return nil, nil, ErrFormat
	}
	sort.Slice(intervals, func(i, j int) bool { return intervals[i].start < intervals[j].start })
	for i := 1; i < len(intervals); i++ {
		if intervals[i].start < intervals[i-1].end {
			return nil, nil, ErrFormat
		}
	}
	// WDC2's explicit list addresses unique physical rows. WDC3+ repeats map IDs.
	if p.IdentityBytes != 0 {
		if uint64(p.IdentityBytes) != uint64(len(ids))*4 {
			return nil, nil, ErrFormat
		}
		start := int64(0)
		if layout.Version == 2 {
			start = count * 6
		}
		for i, id := range ids {
			if binary.LittleEndian.Uint32(aux[start+int64(i)*4:start+int64(i)*4+4]) != id {
				return nil, nil, ErrFormat
			}
		}
	}
	normalized := make([]byte, int64(p.IdentityBytes)+int64(p.Copies)*8+int64(p.RelationBytes))
	copy(normalized[int(p.IdentityBytes):], aux[copyStart:copyStart+int64(p.Copies)*8])
	copy(normalized[int64(p.IdentityBytes)+int64(p.Copies)*8:], aux[relationStart:relationStart+int64(p.RelationBytes)])
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	return ids, normalized, nil
}
