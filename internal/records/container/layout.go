// SPDX-License-Identifier: AGPL-3.0-or-later
// BLTE layout handling adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package container

import (
	"encoding/binary"
	"io"
)

func (d *decoder) layout(src io.Reader, allowance int64, boundedChunks bool) ([]segment, uint64, error) {
	var prefix [8]byte
	if err := exact(src, prefix[:]); err != nil {
		return nil, 0, err
	}
	if string(prefix[:4]) != "BLTE" {
		return nil, 0, ErrMalformed
	}
	header := uint64(binary.BigEndian.Uint32(prefix[4:]))
	var parts []segment
	if header != 0 {
		var descriptor [4]byte
		if err := exact(src, descriptor[:]); err != nil {
			return nil, 0, err
		}
		count := int(descriptor[1])<<16 | int(descriptor[2])<<8 | int(descriptor[3])
		if descriptor[0] != 0x0f || count == 0 || header != 12+uint64(count)*24 {
			return nil, 0, ErrMalformed
		}
		if count > d.limits.Chunks-d.chunks {
			return nil, 0, ErrLimit
		}
		parts = make([]segment, count)
		var total int64
		for i := range parts {
			var entry [24]byte
			if err := exact(src, entry[:]); err != nil {
				return nil, 0, err
			}
			parts[i].encoded = binary.BigEndian.Uint32(entry[:4])
			parts[i].decoded = binary.BigEndian.Uint32(entry[4:8])
			copy(parts[i].digest[:], entry[8:])
			if parts[i].encoded == 0 {
				return nil, 0, ErrMalformed
			}
			total += int64(parts[i].decoded)
			if (boundedChunks && (int64(parts[i].encoded) > d.limits.ChunkBytes || int64(parts[i].decoded) > d.limits.ChunkBytes)) || total > allowance {
				return nil, 0, ErrLimit
			}
		}
	} else {
		parts = make([]segment, 1)
	}
	return parts, header, nil
}
