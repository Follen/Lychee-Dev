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

var ErrEncodingMissing = errors.New("records.encoding_not_found")

// EncodedRecord describes physical BLTE bytes, excluding the local archive's
// preamble. Specification is an index, not an interpreted encoding recipe.
type EncodedRecord struct {
	EncodingKey   string `json:"encodingKey"`
	EncodedBytes  int64  `json:"encodedBytes"`
	Specification uint32 `json:"specification"`
}

// FindEncoding reads the EKey directory and one independently checked page.
// No shared mutable cache is created; concurrent reads retain their own buffers.
func (e *EncodingIndex) FindEncoding(ctx context.Context, encodingKey string) (EncodedRecord, error) {
	if err := ctx.Err(); err != nil {
		return EncodedRecord{}, err
	}
	if !metadataKey(encodingKey) {
		return EncodedRecord{}, ErrMetadataFormat
	}
	if e.encodedPageCount == 0 {
		return EncodedRecord{}, ErrEncodingMissing
	}
	start := e.pagesStart + int64(e.pageCount)*e.pageSize
	directory, err := e.source.ReadSpan(ctx, start, int64(e.encodedPageCount)*32)
	if err != nil {
		return EncodedRecord{}, err
	}
	for i := 0; i < e.encodedPageCount; i++ {
		if i&1023 == 0 {
			if err := ctx.Err(); err != nil {
				return EncodedRecord{}, err
			}
		}
		if i > 0 && bytes.Compare(directory[(i-1)*32:(i-1)*32+16], directory[i*32:i*32+16]) >= 0 {
			return EncodedRecord{}, ErrMetadataFormat
		}
		if [16]byte(directory[i*32+16:(i+1)*32]) == ([16]byte{}) {
			return EncodedRecord{}, container.ErrIntegrity
		}
	}
	key, _ := hex.DecodeString(encodingKey)
	page := sort.Search(e.encodedPageCount, func(i int) bool { return bytes.Compare(directory[i*32:i*32+16], key) > 0 }) - 1
	if page < 0 {
		return EncodedRecord{}, ErrEncodingMissing
	}
	expected := [md5.Size]byte(directory[page*32+16 : (page+1)*32])
	raw, err := e.source.ReadCheckedSpan(ctx, start+int64(e.encodedPageCount)*32+int64(page)*e.encodedPageSize, e.encodedPageSize, expected)
	if err != nil {
		return EncodedRecord{}, err
	}
	var result EncodedRecord
	var previous []byte
	for pos := 0; pos < len(raw); {
		// Remaining all-zero bytes are padding, not a zero-sized record.
		if allZero(raw[pos:]) {
			break
		}
		if len(raw)-pos < 25 {
			return EncodedRecord{}, ErrMetadataFormat
		}
		current := raw[pos : pos+16]
		if previous == nil {
			if !bytes.Equal(current, directory[page*32:page*32+16]) {
				return EncodedRecord{}, ErrMetadataFormat
			}
		} else if bytes.Compare(previous, current) >= 0 {
			return EncodedRecord{}, ErrMetadataFormat
		}
		if page+1 < e.encodedPageCount && bytes.Compare(current, directory[(page+1)*32:(page+1)*32+16]) >= 0 {
			return EncodedRecord{}, ErrMetadataFormat
		}
		size := int64(raw[pos+20])<<32 | int64(binary.BigEndian.Uint32(raw[pos+21:pos+25]))
		if size < 8 {
			return EncodedRecord{}, ErrMetadataFormat
		}
		if bytes.Equal(current, key) {
			result = EncodedRecord{EncodingKey: encodingKey, EncodedBytes: size, Specification: binary.BigEndian.Uint32(raw[pos+16 : pos+20])}
		}
		previous = current
		pos += 25
	}
	if err := ctx.Err(); err != nil {
		return EncodedRecord{}, err
	}
	if previous == nil {
		return EncodedRecord{}, ErrMetadataFormat
	}
	if result.EncodingKey == "" {
		return EncodedRecord{}, ErrEncodingMissing
	}
	return result, nil
}

func allZero(raw []byte) bool {
	for _, b := range raw {
		if b != 0 {
			return false
		}
	}
	return true
}
