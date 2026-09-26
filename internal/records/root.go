// SPDX-License-Identifier: MIT
// Root layout adapted from wowdata; see LICENSE.
package records

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"io"
)

type RootLimits struct {
	Bytes   int64
	Records uint64
	Groups  int
	Matches int
}

// RootRecord retains every variant. Locale and content selection belongs to the
// pinned data resolver, not to the format reader. NameHash is meaningful only
// when HasNameHash is true; it is not a resolved filename.
type RootRecord struct {
	FileDataID   uint32 `json:"fileDataID"`
	ContentKey   string `json:"contentKey"`
	LocaleMask   uint32 `json:"localeMask"`
	ContentFlags uint32 `json:"contentFlags"`
	NameHash     uint64 `json:"nameHash"`
	HasNameHash  bool   `json:"hasNameHash"`
	Group        int    `json:"group"`
}

// LookupRoot projects selected IDs from an immutable decoded Root. It checks
// all block extents and delta arrays before returning results, retains no global
// state, and never returns partial matches on failure. The caller must verify
// the source content key; structural validity alone does not authenticate it.
func LookupRoot(ctx context.Context, source io.ReaderAt, size int64, ids []uint32, limits RootLimits) ([]RootRecord, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if source == nil || size < 4 {
		return nil, ErrMetadataFormat
	}
	if limits.Bytes <= 0 || limits.Bytes > 1<<40 || size > limits.Bytes || limits.Records == 0 || limits.Records > 1<<32 || limits.Groups <= 0 || limits.Groups > 1<<20 || limits.Matches <= 0 || limits.Matches > 1<<20 || len(ids) == 0 || len(ids) > 4096 {
		return nil, ErrMetadataLimit
	}
	wanted := make(map[uint32]bool, len(ids))
	for _, id := range ids {
		wanted[id] = true
	}
	input := rootInput{ctx: ctx, source: source, size: size}
	var header [24]byte
	if err := input.read(header[:4], 0); err != nil {
		return nil, err
	}
	classic := string(header[:4]) != "TSFM"
	version := uint32(0)
	pos := int64(0)
	allowNameless := false
	if !classic {
		if err := input.read(header[:12], 0); err != nil {
			return nil, err
		}
		second, third := binary.LittleEndian.Uint32(header[4:8]), binary.LittleEndian.Uint32(header[8:12])
		total, named := second, third
		pos = 12
		// In the old header, the same slots are total/named counts. A small
		// total alone must not be mistaken for an extended header size.
		if (second == 20 || second == 24) && (third == 1 || third == 2) {
			if err := input.read(header[:second], 0); err != nil {
				return nil, err
			}
			version, pos = third, int64(second)
			total, named = binary.LittleEndian.Uint32(header[12:16]), binary.LittleEndian.Uint32(header[16:20])
		}
		if named > total {
			return nil, ErrMetadataFormat
		}
		allowNameless = total != named
	}
	var result []RootRecord
	var scanned uint64
	var deltas [64 * 1024]byte
	for group := 0; pos < size; group++ {
		if group >= limits.Groups {
			return nil, ErrMetadataLimit
		}
		flagsBytes := 12
		if version == 2 {
			flagsBytes = 17
		}
		if err := input.read(header[:flagsBytes], pos); err != nil {
			return nil, err
		}
		count := binary.LittleEndian.Uint32(header[:4])
		content, locale := binary.LittleEndian.Uint32(header[4:8]), binary.LittleEndian.Uint32(header[8:12])
		if version == 2 {
			locale = content
			content = binary.LittleEndian.Uint32(header[8:12]) | binary.LittleEndian.Uint32(header[12:16]) | uint32(header[16])<<17
		}
		scanned += uint64(count)
		if scanned > limits.Records {
			return nil, ErrMetadataLimit
		}
		pos += int64(flagsBytes)
		keyStride := int64(16)
		hasName := classic || !(allowNameless && content&0x10000000 != 0)
		if classic {
			keyStride = 24
		}
		recordBytes := int64(20)
		if hasName {
			recordBytes += 8
		}
		if int64(count)*recordBytes > size-pos {
			return nil, ErrMetadataFormat
		}
		keysStart := pos + int64(count)*4
		namesStart := keysStart + int64(count)*keyStride
		var next uint64
		for base := uint32(0); base < count; {
			batch := min(uint32(len(deltas)/4), count-base)
			if err := input.read(deltas[:int(batch)*4], pos+int64(base)*4); err != nil {
				return nil, err
			}
			for i := uint32(0); i < batch; i++ {
				id := next + uint64(binary.LittleEndian.Uint32(deltas[i*4:i*4+4]))
				if id > 0xffffffff {
					return nil, ErrMetadataFormat
				}
				next = id + 1
				if !wanted[uint32(id)] {
					continue
				}
				if len(result) >= limits.Matches {
					return nil, ErrMetadataLimit
				}
				var key [24]byte
				ordinal := int64(base + i)
				if err := input.read(key[:keyStride], keysStart+ordinal*keyStride); err != nil {
					return nil, err
				}
				record := RootRecord{FileDataID: uint32(id), ContentKey: hex.EncodeToString(key[:16]), LocaleMask: locale, ContentFlags: content, HasNameHash: hasName, Group: group}
				if hasName {
					if !classic {
						if err := input.read(key[16:24], namesStart+ordinal*8); err != nil {
							return nil, err
						}
					}
					record.NameHash = binary.LittleEndian.Uint64(key[16:24])
				}
				result = append(result, record)
			}
			base += batch
		}
		pos += int64(count) * recordBytes
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return result, nil
}

type rootInput struct {
	ctx    context.Context
	source io.ReaderAt
	size   int64
}

func (r rootInput) read(dst []byte, offset int64) error {
	if err := r.ctx.Err(); err != nil {
		return err
	}
	if offset < 0 || offset > r.size || int64(len(dst)) > r.size-offset {
		return ErrMetadataFormat
	}
	n, err := r.source.ReadAt(dst, offset)
	if n != len(dst) {
		if err != nil {
			return err
		}
		return io.ErrUnexpectedEOF
	}
	if err != nil && err != io.EOF {
		return err
	}
	return nil
}
