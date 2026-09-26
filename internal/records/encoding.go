// SPDX-License-Identifier: MIT
// Encoding format handling adapted from wowdata; see LICENSE.
package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"sort"

	"github.com/follenfang/lycheedev/internal/records/container"
)

var ErrContentMissing = errors.New("records.content_not_found")

// EncodingIndex retains only the CKey page directory. Readers must keep the
// backing BLTE source open and immutable. OpenLocalObject authenticates its EKey;
// this parser does not independently verify the whole decoded content key.
type EncodingIndex struct {
	source           *container.Ranges
	directory        []byte
	pageSize         int64
	pagesStart       int64
	pageCount        int
	encodedPageSize  int64
	encodedPageCount int
}

type EncodingRecord struct {
	ContentKey   string   `json:"contentKey"`
	DecodedBytes int64    `json:"decodedBytes"`
	EncodingKeys []string `json:"encodingKeys"`
	Page         int      `json:"page"`
}

func OpenEncoding(ctx context.Context, source *container.Ranges) (*EncodingIndex, error) {
	if source == nil || source.Size() < 22 {
		return nil, ErrMetadataFormat
	}
	header, err := source.ReadSpan(ctx, 0, 22)
	if err != nil {
		return nil, err
	}
	if string(header[:2]) != "EN" || header[2] != 1 || header[3] != 16 || header[4] != 16 || header[17] != 0 {
		return nil, ErrMetadataFormat
	}
	pageSize := int64(binary.BigEndian.Uint16(header[5:7])) * 1024
	pageCount := int64(binary.BigEndian.Uint32(header[9:13]))
	encodedPageSize := int64(binary.BigEndian.Uint16(header[7:9])) * 1024
	encodedPageCount := int64(binary.BigEndian.Uint32(header[13:17]))
	specBytes := int64(binary.BigEndian.Uint32(header[18:22]))
	if pageSize == 0 || pageSize > 1<<20 || pageCount > 1<<20 || specBytes > 64<<20 {
		return nil, ErrMetadataLimit
	}
	if encodedPageCount > 1<<20 || encodedPageSize > 1<<20 || (encodedPageCount > 0 && encodedPageSize == 0) {
		return nil, ErrMetadataLimit
	}
	directoryStart := 22 + specBytes
	pagesStart := directoryStart + pageCount*32
	if pagesStart > source.Size() || pageCount*pageSize > source.Size()-pagesStart {
		return nil, ErrMetadataFormat
	}
	encodedStart := pagesStart + pageCount*pageSize
	if encodedPageCount*(32+encodedPageSize) > source.Size()-encodedStart {
		return nil, ErrMetadataFormat
	}
	directory, err := source.ReadSpan(ctx, directoryStart, pageCount*32)
	if err != nil {
		return nil, err
	}
	for i := int64(0); i < pageCount; i++ {
		if i&1023 == 0 {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
		}
		entry := directory[i*32 : (i+1)*32]
		if i > 0 && bytes.Compare(directory[(i-1)*32:(i-1)*32+16], entry[:16]) >= 0 {
			return nil, ErrMetadataFormat
		}
		if [md5.Size]byte(entry[16:]) == ([md5.Size]byte{}) {
			return nil, container.ErrIntegrity
		}
	}
	return &EncodingIndex{source: source, directory: directory, pageSize: pageSize, pagesStart: pagesStart, pageCount: int(pageCount), encodedPageSize: encodedPageSize, encodedPageCount: int(encodedPageCount)}, nil
}

// FindContent fetches one candidate page and checks its full checksum, record
// bounds and key ordering before returning every encoding alternative. It does
// not fetch unrelated pages or silently choose the first EKey alternative.
func (e *EncodingIndex) FindContent(ctx context.Context, contentKey string) (EncodingRecord, error) {
	if err := ctx.Err(); err != nil {
		return EncodingRecord{}, err
	}
	if !metadataKey(contentKey) {
		return EncodingRecord{}, ErrMetadataFormat
	}
	key, _ := hex.DecodeString(contentKey)
	page := sort.Search(e.pageCount, func(i int) bool { return bytes.Compare(e.directory[i*32:i*32+16], key) > 0 }) - 1
	if page < 0 {
		return EncodingRecord{}, ErrContentMissing
	}
	var expected [md5.Size]byte
	copy(expected[:], e.directory[page*32+16:(page+1)*32])
	raw, err := e.source.ReadCheckedSpan(ctx, e.pagesStart+int64(page)*e.pageSize, e.pageSize, expected)
	if err != nil {
		return EncodingRecord{}, err
	}
	var result EncodingRecord
	var previous []byte
	for pos := 0; pos < len(raw); {
		count := int(raw[pos])
		pos++
		if count == 0 {
			for _, v := range raw[pos:] {
				if v != 0 {
					return EncodingRecord{}, ErrMetadataFormat
				}
			}
			break
		}
		if len(raw)-pos < 21+count*16 {
			return EncodingRecord{}, ErrMetadataFormat
		}
		size := int64(raw[pos])<<32 | int64(binary.BigEndian.Uint32(raw[pos+1:pos+5]))
		current := raw[pos+5 : pos+21]
		if previous == nil {
			if !bytes.Equal(current, e.directory[page*32:page*32+16]) {
				return EncodingRecord{}, ErrMetadataFormat
			}
		} else if bytes.Compare(previous, current) >= 0 {
			return EncodingRecord{}, ErrMetadataFormat
		}
		if page+1 < e.pageCount && bytes.Compare(current, e.directory[(page+1)*32:(page+1)*32+16]) >= 0 {
			return EncodingRecord{}, ErrMetadataFormat
		}
		previous = current
		if bytes.Equal(current, key) {
			result = EncodingRecord{ContentKey: contentKey, DecodedBytes: size, Page: page, EncodingKeys: make([]string, count)}
			for i := 0; i < count; i++ {
				result.EncodingKeys[i] = hex.EncodeToString(raw[pos+21+i*16 : pos+21+(i+1)*16])
			}
		}
		pos += 21 + count*16
	}
	if err := ctx.Err(); err != nil {
		return EncodingRecord{}, err
	}
	if previous == nil {
		return EncodingRecord{}, ErrMetadataFormat
	}
	if result.ContentKey == "" {
		return EncodingRecord{}, ErrContentMissing
	}
	return result, nil
}
