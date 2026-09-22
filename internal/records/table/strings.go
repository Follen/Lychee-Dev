package table

import (
	"bytes"
	"context"
	"encoding/binary"
	"unicode/utf8"
)

// Strings interprets a schema-selected direct 32-bit pointer column. WDC2 uses
// physical relative positions; WDC3+ uses concatenated records/string tables.
// The byte budget covers all returned UTF-8 bytes, excluding terminators.
// The source must already have passed full content verification, like Lookup.
func (r *Records) Strings(ctx context.Context, id uint32, column int, elements uint32, maxBytes int) ([]string, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if r.columns.layout.Flags&1 != 0 {
		return nil, ErrUnsupported
	}
	if maxBytes <= 0 || maxBytes > 1<<20 || elements == 0 || elements > 65536 {
		return nil, ErrLimit
	}
	if column < 0 || column >= len(r.columns.programs) {
		return nil, ErrFormat
	}
	storage := r.columns.programs[column].storage
	if storage.Codec != 0 {
		return nil, ErrUnsupported
	}
	if storage.BitOffset%8 != 0 || uint32(storage.BitWidth) != elements*32 {
		return nil, ErrFormat
	}
	row, err := r.Lookup(ctx, id)
	if err != nil {
		return nil, err
	}
	return r.recordStrings(ctx, row, storage, elements, maxBytes)
}

func (r *Records) recordStrings(ctx context.Context, row Record, storage Storage, elements uint32, maxBytes int) ([]string, error) {
	field := int64(storage.BitOffset) / 8
	if field+int64(elements)*4 > int64(len(row.Data)) {
		return nil, ErrFormat
	}
	layout := r.columns.layout
	var priorRows int64
	for i := 0; i < row.Partition; i++ {
		priorRows += int64(layout.Partitions[i].Rows)
	}
	result := make([]string, int(elements))
	remaining := maxBytes
	for i := range result {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		position := field + int64(i)*4
		pointer := binary.LittleEndian.Uint32(row.Data[position : position+4])
		if pointer == 0 {
			continue
		}
		target := row.Offset + position + int64(int32(pointer))
		if layout.Version >= 3 {
			localRow := row.Offset - int64(layout.Partitions[row.Partition].Offset)
			target = (priorRows-int64(layout.Rows))*int64(layout.Stride) + localRow + position + int64(int32(pointer))
		}
		var base int64
		found := false
		for _, p := range layout.Partitions {
			start := int64(p.Offset) + int64(p.Rows)*int64(layout.Stride)
			local := target - start
			if layout.Version >= 3 {
				local = target - base
			}
			base += int64(p.StringBytes)
			if local < 0 || local >= int64(p.StringBytes) {
				continue
			}
			value, err := r.terminatedString(ctx, start+local, start+int64(p.StringBytes), remaining)
			if err != nil {
				return nil, err
			}
			result[i] = value
			remaining -= len(value)
			found = true
			break
		}
		if !found {
			return nil, ErrFormat
		}
	}
	return result, nil
}

func (r *Records) terminatedString(ctx context.Context, start, end int64, budget int) (string, error) {
	var result []byte
	for start < end {
		length := min(int64(256), end-start, int64(budget-len(result))+1)
		if length <= 0 {
			return "", ErrLimit
		}
		chunk, err := r.read(ctx, start, length)
		if err != nil {
			return "", err
		}
		if zero := bytes.IndexByte(chunk, 0); zero >= 0 {
			result = append(result, chunk[:zero]...)
			if !utf8.Valid(result) {
				return "", ErrFormat
			}
			return string(result), nil
		}
		result = append(result, chunk...)
		if len(result) > budget {
			return "", ErrLimit
		}
		start += length
	}
	return "", ErrFormat
}
