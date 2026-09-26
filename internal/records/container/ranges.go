// SPDX-License-Identifier: MIT
// BLTE range handling adapted from wowdata; see LICENSE.
package container

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"sort"
)

// Ranges owns an immutable chunk directory, not the source or a decoded cache.
// The caller must keep source bytes immutable and authenticate the header using
// its external CASC encoding identity. Chunk MD5 is not cryptographic provenance.
// Calls may run concurrently when source and keys support concurrent use.
type Ranges struct {
	source         io.ReaderAt
	limits         Limits
	keys           KeyLookup
	parts          []segment
	encodedOffsets []int64
	decodedOffsets []int64
}

// OpenRanges reads only the framed directory. encodedSize is the exact extent,
// not an archive size; use a section reader for an object inside an archive.
// Headerless objects require sequential Decode because their output size is not
// declared. Total/count budgets cover the whole directory. ChunkBytes applies
// only to visited chunks and each returned buffer; large unvisited chunks do not
// prevent bounded metadata reads. Blocking reads need caller-side timeouts.
func OpenRanges(ctx context.Context, source io.ReaderAt, encodedSize int64, limits Limits, keys KeyLookup) (*Ranges, error) {
	if source == nil || !limits.valid() || encodedSize > limits.EncodedBytes {
		return nil, ErrLimit
	}
	if encodedSize < 8 {
		return nil, ErrMalformed
	}
	var prefix [8]byte
	if err := at(ctx, source, prefix[:], 0); err != nil {
		return nil, err
	}
	if string(prefix[:4]) != "BLTE" {
		return nil, ErrMalformed
	}
	headerSize := int64(binary.BigEndian.Uint32(prefix[4:]))
	if headerSize == 0 {
		return nil, ErrUnsupported
	}
	if headerSize < 12 || headerSize > encodedSize {
		return nil, ErrMalformed
	}
	if headerSize > 12+int64(limits.Chunks)*24 {
		return nil, ErrLimit
	}
	header := make([]byte, int(headerSize))
	copy(header, prefix[:])
	if err := at(ctx, source, header[8:], 8); err != nil {
		return nil, err
	}
	d := decoder{ctx: ctx, limits: limits}
	parts, _, err := d.layout(bytes.NewReader(header), limits.DecodedBytes, false)
	if err != nil {
		return nil, err
	}
	r := &Ranges{source: source, limits: limits, keys: keys, parts: parts, encodedOffsets: make([]int64, len(parts)+1), decodedOffsets: make([]int64, len(parts)+1)}
	r.encodedOffsets[0] = headerSize
	for i, part := range parts {
		r.encodedOffsets[i+1] = r.encodedOffsets[i] + int64(part.encoded)
		r.decodedOffsets[i+1] = r.decodedOffsets[i] + int64(part.decoded)
	}
	if r.encodedOffsets[len(parts)] != encodedSize {
		return nil, fmt.Errorf("%w: encoded extent mismatch", ErrMalformed)
	}
	return r, nil
}

func (r *Ranges) Size() int64 { return r.decodedOffsets[len(r.parts)] }

// ReadCheckedSpan validates an independently supplied logical-range checksum.
// For a range wholly inside an N block, this reads only that range and the mode
// byte, not the entire block. The caller must obtain expected from authenticated
// format metadata. This verifies the logical range, NOT untouched block bytes.
// Other encodings retain ReadSpan's full intersecting-block checks and budgets.
func (r *Ranges) ReadCheckedSpan(ctx context.Context, offset, length int64, expected [md5.Size]byte) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if expected == ([md5.Size]byte{}) {
		return nil, ErrIntegrity
	}
	if offset < 0 || length < 0 || offset > r.Size() || length > r.Size()-offset {
		return nil, ErrMalformed
	}
	if length > r.limits.ChunkBytes {
		return nil, ErrLimit
	}
	first := sort.Search(len(r.parts), func(i int) bool { return r.decodedOffsets[i+1] > offset })
	var result []byte
	if length > 0 && first < len(r.parts) && offset+length <= r.decodedOffsets[first+1] {
		var mode [1]byte
		if err := at(ctx, r.source, mode[:], r.encodedOffsets[first]); err != nil {
			return nil, err
		}
		if mode[0] == 'N' {
			if uint64(r.parts[first].encoded) != uint64(r.parts[first].decoded)+1 {
				return nil, ErrIntegrity
			}
			result = make([]byte, int(length))
			if err := at(ctx, r.source, result, r.encodedOffsets[first]+1+offset-r.decodedOffsets[first]); err != nil {
				return nil, err
			}
		}
	}
	if result == nil {
		var err error
		result, err = r.ReadSpan(ctx, offset, length)
		if err != nil {
			return nil, err
		}
	}
	if md5.Sum(result) != expected {
		return nil, ErrIntegrity
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

// ReadSpan returns owned bytes or nil on any failure, never a partial result.
// Only intersecting chunks are fetched and verified. A visited chunk without an
// embedded checksum is rejected (sequential Decode plus a full content key is
// still available for that case). Unvisited chunks and the
// whole decoded content key remain unverified; the result is not a full capture.
func (r *Ranges) ReadSpan(ctx context.Context, offset, length int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if offset < 0 || length < 0 || offset > r.Size() || length > r.Size()-offset {
		return nil, fmt.Errorf("%w: decoded range", ErrMalformed)
	}
	if length > r.limits.ChunkBytes {
		return nil, ErrLimit
	}
	output := make([]byte, int(length))
	if length == 0 {
		return output, nil
	}
	first := sort.Search(len(r.parts), func(i int) bool { return r.decodedOffsets[i+1] > offset })
	d := decoder{ctx: ctx, limits: r.limits, keys: r.keys, chunks: len(r.parts)}
	end := offset + length
	for i := first; i < len(r.parts) && r.decodedOffsets[i] < end; i++ {
		part := r.parts[i]
		if part.decoded == 0 {
			continue
		}
		if part.digest == ([md5.Size]byte{}) {
			return nil, ErrIntegrity
		}
		if int64(part.encoded) > r.limits.ChunkBytes || int64(part.decoded) > r.limits.ChunkBytes {
			return nil, ErrLimit
		}
		raw := make([]byte, int(part.encoded))
		if err := at(ctx, r.source, raw, r.encodedOffsets[i]); err != nil {
			return nil, err
		}
		if md5.Sum(raw) != part.digest {
			return nil, fmt.Errorf("%w: chunk %d checksum", ErrIntegrity, i)
		}
		decoded, err := d.unpack(raw, 1, r.limits.ChunkBytes, i)
		if err != nil {
			return nil, fmt.Errorf("chunk %d: %w", i, err)
		}
		if int64(len(decoded)) != int64(part.decoded) {
			return nil, fmt.Errorf("%w: chunk %d decoded length", ErrIntegrity, i)
		}
		start, stop := max(offset, r.decodedOffsets[i]), min(end, r.decodedOffsets[i+1])
		copy(output[start-offset:stop-offset], decoded[start-r.decodedOffsets[i]:stop-r.decodedOffsets[i]])
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return output, nil
}

func at(ctx context.Context, source io.ReaderAt, output []byte, offset int64) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	n, err := source.ReadAt(output, offset)
	if cancel := ctx.Err(); cancel != nil {
		return cancel
	}
	if n < 0 || n > len(output) {
		return ErrMalformed
	}
	if err != nil && !errors.Is(err, io.EOF) {
		return err
	}
	if n != len(output) {
		return fmt.Errorf("%w: short range read", ErrMalformed)
	}
	return nil
}
