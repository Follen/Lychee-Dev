// SPDX-License-Identifier: MIT
// CASC layout adapted from wowdata; format validation follows CascLib's
// documented v7 guarded directory. See THIRD_PARTY_NOTICES.md.
package archive

import (
	"bufio"
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
)

var (
	ErrIndexFormat    = errors.New("archive.invalid_index")
	ErrIndexIntegrity = errors.New("archive.index_integrity")
	ErrIndexLimit     = errors.New("archive.index_limit")
)

// Span is a candidate archive extent, including the 30-byte local preamble.
// A nine-byte index prefix is not a full encoding identity. Validate the full
// BLTE encoding key before trusting a candidate; collisions are retained.
type Span struct {
	Archive uint16 `json:"archive"`
	Offset  int64  `json:"offset"`
	Bytes   int64  `json:"bytes"`
}

// FindIndexSpans scans one bounded, v7 contiguous local index. Both guarded
// checksums and sorted key order must verify before any candidates are returned.
// Memory is bounded to a read buffer and at most 64 collisions, not all records.
// The caller selects an explicit index generation/bucket; no old-cache discovery
// or nondeterministic first-match selection occurs here. Other layouts fail.
func FindIndexSpans(ctx context.Context, source io.Reader, size int64, key [16]byte, bucket byte) ([]Span, error) {
	if source == nil || size < 40 || bucket > 15 {
		return nil, ErrIndexFormat
	}
	if size > 64<<20 {
		return nil, ErrIndexLimit
	}
	r := bufio.NewReaderSize(io.LimitReader(source, size), 64<<10)
	read := func(p []byte) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		_, err := io.ReadFull(r, p)
		if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
			return ErrIndexFormat
		}
		return err
	}
	var header [40]byte
	if err := read(header[:]); err != nil {
		return nil, err
	}
	if binary.LittleEndian.Uint32(header[:4]) != 16 {
		return nil, ErrIndexFormat
	}
	meta := header[8:24]
	if binary.LittleEndian.Uint16(meta) != 7 || meta[2] != bucket || meta[3] != 0 || meta[4] != 4 || meta[5] != 5 || meta[6] != 9 || meta[7] == 0 || meta[7] > 39 {
		return nil, ErrIndexFormat
	}
	hash, _ := guardSum(meta, 0, 0)
	if hash != binary.LittleEndian.Uint32(header[4:8]) {
		return nil, ErrIndexIntegrity
	}
	length := int64(binary.LittleEndian.Uint32(header[32:36]))
	if length%18 != 0 || length > size-40 {
		return nil, ErrIndexFormat
	}
	if length/18 > 1<<20 {
		return nil, ErrIndexLimit
	}
	maxOffset := binary.LittleEndian.Uint64(meta[8:])
	if maxOffset == 0 {
		return nil, ErrIndexFormat
	}
	// MaxFileOffset in installed indices describes the combined storage space,
	// not a single archive size. SegmentBits is the per-archive offset width.
	segmentLimit := uint64(1) << meta[7]
	var primary, secondary, alternate uint32
	var previous [9]byte
	result := make([]Span, 0)
	for i := int64(0); i < length/18; i++ {
		var entry [18]byte
		if err := read(entry[:]); err != nil {
			return nil, err
		}
		if i > 0 && bytes.Compare(previous[:], entry[:9]) > 0 {
			return nil, ErrIndexFormat
		}
		copy(previous[:], entry[:9])
		primary, secondary = guardSum(entry[:], primary, secondary)
		alternate, _ = guardSum(entry[:], alternate, 0)
		if !bytes.Equal(entry[:9], key[:9]) {
			continue
		}
		location := uint64(entry[9])<<32 | uint64(binary.BigEndian.Uint32(entry[10:14]))
		archive := location >> meta[7]
		offset := location & ((uint64(1) << meta[7]) - 1)
		stored := uint64(binary.LittleEndian.Uint32(entry[14:]))
		if archive > 4095 || stored < 38 || stored > segmentLimit-offset {
			return nil, ErrIndexFormat
		}
		if len(result) == 64 {
			return nil, ErrIndexLimit
		}
		result = append(result, Span{Archive: uint16(archive), Offset: int64(offset), Bytes: int64(stored)})
	}
	expected := binary.LittleEndian.Uint32(header[36:])
	if expected != primary && expected != alternate {
		return nil, ErrIndexIntegrity
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

func (s Span) Filename() string { return fmt.Sprintf("data.%03d", s.Archive) }
