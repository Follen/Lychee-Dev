// SPDX-License-Identifier: AGPL-3.0-or-later
// TACT index layouts adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"io"
	"sort"
)

type cdnIndex struct {
	source                              io.ReaderAt
	pageBytes, stride, perPage, entries int
	offsetBytes                         int
	keys, hashes                        []byte
}

type cdnSpan struct {
	Object string `json:"object"`
	Offset int64  `json:"offset"`
	Bytes  int64  `json:"bytes"`
}

// Only the footer, authenticated TOC and selected page are read. The 28-byte
// v1 footer identity is the index filename, not MD5 of the entire index file.
func openCDNIndex(ctx context.Context, source io.ReaderAt, size int64, key string) (*cdnIndex, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if source == nil || !metadataKey(key) || size < 28 {
		return nil, ErrMetadataFormat
	}
	if size > 1<<30 {
		return nil, ErrMetadataLimit
	}
	foot := make([]byte, 28)
	if _, err := source.ReadAt(foot, size-28); err != nil {
		return nil, err
	}
	sum := md5.Sum(foot)
	if hex.EncodeToString(sum[:]) != key {
		return nil, ErrConfigurationIntegrity
	}
	if foot[8] != 1 || foot[9] != 0 || foot[10] != 0 || foot[13] != 4 || foot[14] != 16 || foot[15] != 8 || (foot[12] != 0 && foot[12] != 4 && foot[12] != 5 && foot[12] != 6) {
		return nil, ErrMetadataFormat
	}
	check := make([]byte, 20)
	copy(check, foot[8:20])
	sum = md5.Sum(check)
	if !bytes.Equal(sum[:8], foot[20:]) {
		return nil, ErrConfigurationIntegrity
	}
	pageBytes := int(foot[11]) * 1024
	stride := 20 + int(foot[12])
	entries := uint64(binary.LittleEndian.Uint32(foot[16:20]))
	if pageBytes == 0 || entries > 10000000 {
		return nil, ErrMetadataLimit
	}
	perPage := pageBytes / stride
	pages := (int64(entries) + int64(perPage) - 1) / int64(perPage)
	if pages*(int64(pageBytes)+24)+28 != size {
		return nil, ErrMetadataFormat
	}
	toc := make([]byte, pages*24)
	if _, err := source.ReadAt(toc, pages*int64(pageBytes)); err != nil {
		return nil, err
	}
	sum = md5.Sum(toc)
	if !bytes.Equal(sum[:8], foot[:8]) {
		return nil, ErrConfigurationIntegrity
	}
	keys, hashes := toc[:pages*16], toc[pages*16:]
	for i := int64(0); i < pages; i++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if bytes.Equal(keys[i*16:i*16+16], make([]byte, 16)) || i > 0 && bytes.Compare(keys[(i-1)*16:i*16], keys[i*16:i*16+16]) >= 0 {
			return nil, ErrMetadataFormat
		}
	}
	return &cdnIndex{source: source, pageBytes: pageBytes, stride: stride, perPage: perPage, entries: int(entries), offsetBytes: int(foot[12]), keys: keys, hashes: hashes}, nil
}

func (index *cdnIndex) locate(ctx context.Context, key string, archives []string, object string) (cdnSpan, error) {
	if err := ctx.Err(); err != nil {
		return cdnSpan{}, err
	}
	if !metadataKey(key) {
		return cdnSpan{}, ErrMetadataFormat
	}
	needle, _ := hex.DecodeString(key)
	pages := len(index.keys) / 16
	p := sort.Search(pages, func(i int) bool { return bytes.Compare(index.keys[i*16:i*16+16], needle) >= 0 })
	if p == pages {
		return cdnSpan{}, ErrContentMissing
	}
	page := make([]byte, index.pageBytes)
	if _, err := index.source.ReadAt(page, int64(p*index.pageBytes)); err != nil {
		return cdnSpan{}, err
	}
	sum := md5.Sum(page)
	if !bytes.Equal(sum[:8], index.hashes[p*8:p*8+8]) {
		return cdnSpan{}, ErrConfigurationIntegrity
	}
	count := min(index.perPage, index.entries-p*index.perPage)
	var found cdnSpan
	for i := 0; i < count; i++ {
		if err := ctx.Err(); err != nil {
			return cdnSpan{}, err
		}
		entry := page[i*index.stride : (i+1)*index.stride]
		if bytes.Equal(entry[:16], make([]byte, 16)) || i > 0 && bytes.Compare(page[(i-1)*index.stride:(i-1)*index.stride+16], entry[:16]) >= 0 || i == 0 && p > 0 && bytes.Compare(index.keys[(p-1)*16:p*16], entry[:16]) >= 0 {
			return cdnSpan{}, ErrMetadataFormat
		}
		if bytes.Equal(entry[:16], needle) {
			found = cdnSpan{Object: object, Bytes: int64(binary.BigEndian.Uint32(entry[16:20]))}
			if index.offsetBytes == 0 {
				found.Object = key
			} else {
				found.Offset = int64(binary.BigEndian.Uint32(entry[len(entry)-4:]))
				if index.offsetBytes > 4 {
					ordinal := int(entry[20])
					if index.offsetBytes == 6 {
						ordinal = int(binary.BigEndian.Uint16(entry[20:22]))
					}
					if ordinal >= len(archives) {
						return cdnSpan{}, ErrMetadataFormat
					}
					found.Object = archives[ordinal]
				}
			}
			if found.Bytes < 8 || !metadataKey(found.Object) {
				return cdnSpan{}, ErrMetadataFormat
			}
		}
	}
	if count == 0 || !bytes.Equal(page[(count-1)*index.stride:(count-1)*index.stride+16], index.keys[p*16:p*16+16]) {
		return cdnSpan{}, ErrMetadataFormat
	}
	for _, b := range page[count*index.stride:] {
		if b != 0 {
			return cdnSpan{}, ErrMetadataFormat
		}
	}
	if found.Object == "" {
		return cdnSpan{}, ErrContentMissing
	}
	return found, nil
}
