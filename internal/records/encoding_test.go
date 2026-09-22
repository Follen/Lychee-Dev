// SPDX-License-Identifier: AGPL-3.0-or-later
package records_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
)

const encodingTestPageSize = 1024

var encodingTestLimits = container.Limits{
	EncodedBytes: 64 << 20,
	DecodedBytes: 64 << 20,
	ChunkBytes:   64 << 20,
	Chunks:       65536,
	Depth:        8,
}

type encodingTestEntry struct {
	contentKey   [16]byte
	decodedSize  uint64
	encodingKeys [][16]byte
}

type encodingTestPage struct {
	directoryKey [16]byte
	body         []byte
}

type encodingTestObject struct {
	logical    []byte
	raw        []byte
	pagesStart int
}

func testKey(first byte) [16]byte {
	var key [16]byte
	key[0] = first
	key[15] = first
	return key
}

func testKeyString(key [16]byte) string {
	return hex.EncodeToString(key[:])
}

func emptyEncodingPage() []byte {
	return make([]byte, encodingTestPageSize)
}

func encodingPage(t *testing.T, entries ...encodingTestEntry) []byte {
	t.Helper()
	page := emptyEncodingPage()
	position := 0
	for _, entry := range entries {
		if len(entry.encodingKeys) > 255 {
			t.Fatalf("encoding test entry has too many EKeys: %d", len(entry.encodingKeys))
		}
		entryBytes := 22 + len(entry.encodingKeys)*16
		if position+entryBytes > len(page) {
			t.Fatalf("encoding test page entry does not fit: position=%d bytes=%d", position, entryBytes)
		}
		page[position] = byte(len(entry.encodingKeys))
		putUint40(page[position+1:position+6], entry.decodedSize)
		copy(page[position+6:position+22], entry.contentKey[:])
		position += 22
		for _, encodingKey := range entry.encodingKeys {
			copy(page[position:position+16], encodingKey[:])
			position += 16
		}
	}
	return page
}

func putUint40(dst []byte, value uint64) {
	for index := 4; index >= 0; index-- {
		dst[index] = byte(value)
		value >>= 8
	}
}

func buildEncodingObject(t *testing.T, pages ...encodingTestPage) encodingTestObject {
	t.Helper()
	pagesStart := 22 + len(pages)*32
	logical := make([]byte, pagesStart+len(pages)*encodingTestPageSize)
	copy(logical[:2], "EN")
	logical[2] = 1
	logical[3] = 16
	logical[4] = 16
	binary.BigEndian.PutUint16(logical[5:7], 1)
	binary.BigEndian.PutUint16(logical[7:9], 1)
	binary.BigEndian.PutUint32(logical[9:13], uint32(len(pages)))
	binary.BigEndian.PutUint32(logical[13:17], 0)
	logical[17] = 0
	binary.BigEndian.PutUint32(logical[18:22], 0)

	for index, page := range pages {
		if len(page.body) != encodingTestPageSize {
			t.Fatalf("page %d length = %d, want %d", index, len(page.body), encodingTestPageSize)
		}
		directoryEntry := 22 + index*32
		copy(logical[directoryEntry:directoryEntry+16], page.directoryKey[:])
		digest := md5.Sum(page.body)
		if digest == ([md5.Size]byte{}) {
			t.Fatal("test page unexpectedly has a zero MD5")
		}
		copy(logical[directoryEntry+16:directoryEntry+32], digest[:])
		pageOffset := pagesStart + index*encodingTestPageSize
		copy(logical[pageOffset:pageOffset+encodingTestPageSize], page.body)
	}
	return encodingTestObject{logical: logical, raw: frameEncoding(logical), pagesStart: pagesStart}
}

func frameEncoding(decoded []byte) []byte {
	pageCount := int(binary.BigEndian.Uint32(decoded[9:13]))
	pagesStart := 22 + pageCount*32
	blocks := [][]byte{decoded[:pagesStart]}
	if pagesStart < len(decoded) {
		blocks = append(blocks, decoded[pagesStart:])
	}
	headerSize := 12 + 24*len(blocks)
	encodedSize := headerSize
	for _, payload := range blocks {
		encodedSize += 1 + len(payload)
	}
	framed := make([]byte, encodedSize)
	copy(framed[:4], "BLTE")
	binary.BigEndian.PutUint32(framed[4:8], uint32(headerSize))
	framed[8] = 0x0f
	framed[9] = byte(len(blocks) >> 16)
	framed[10] = byte(len(blocks) >> 8)
	framed[11] = byte(len(blocks))
	dataOffset := headerSize
	for index, payload := range blocks {
		block := make([]byte, 1+len(payload))
		block[0] = 'N'
		copy(block[1:], payload)
		entry := 12 + index*24
		binary.BigEndian.PutUint32(framed[entry:entry+4], uint32(len(block)))
		binary.BigEndian.PutUint32(framed[entry+4:entry+8], uint32(len(payload)))
		digest := md5.Sum(block)
		if digest == ([md5.Size]byte{}) {
			panic("test BLTE block unexpectedly has a zero MD5")
		}
		copy(framed[entry+8:entry+24], digest[:])
		copy(framed[dataOffset:], block)
		dataOffset += len(block)
	}
	return framed
}

func openEncodingTestObject(t *testing.T, raw []byte) *records.EncodingIndex {
	t.Helper()
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(raw), int64(len(raw)), encodingTestLimits, nil)
	if err != nil {
		t.Fatalf("OpenRanges: %v", err)
	}
	index, err := records.OpenEncoding(context.Background(), ranges)
	if err != nil {
		t.Fatalf("OpenEncoding: %v", err)
	}
	return index
}

func openEncodingFromLogical(t *testing.T, logical []byte) (*records.EncodingIndex, error) {
	t.Helper()
	raw := frameEncoding(logical)
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(raw), int64(len(raw)), encodingTestLimits, nil)
	if err != nil {
		t.Fatalf("OpenRanges: %v", err)
	}
	return records.OpenEncoding(context.Background(), ranges)
}

func TestEncodingFindContentUsesExactMappingAndRetainsMultipleEKeys(t *testing.T) {
	contentKey := testKey(0x20)
	firstEncodingKey := testKey(0xa0)
	secondEncodingKey := testKey(0xb0)
	object := buildEncodingObject(t, encodingTestPage{
		directoryKey: contentKey,
		body: encodingPage(t,
			encodingTestEntry{
				contentKey:   contentKey,
				decodedSize:  0x100000007,
				encodingKeys: [][16]byte{firstEncodingKey, secondEncodingKey},
			},
		),
	})

	index := openEncodingTestObject(t, object.raw)
	got, err := index.FindContent(context.Background(), testKeyString(contentKey))
	if err != nil {
		t.Fatalf("FindContent: %v", err)
	}
	if got.ContentKey != testKeyString(contentKey) || got.DecodedBytes != 0x100000007 || got.Page != 0 {
		t.Fatalf("record = %#v, want key=%s decoded=%d page=0", got, testKeyString(contentKey), 0x100000007)
	}
	wantKeys := []string{testKeyString(firstEncodingKey), testKeyString(secondEncodingKey)}
	if len(got.EncodingKeys) != len(wantKeys) || got.EncodingKeys[0] != wantKeys[0] || got.EncodingKeys[1] != wantKeys[1] {
		t.Fatalf("encoding keys = %#v, want %#v", got.EncodingKeys, wantKeys)
	}
}

func TestEncodingFindContentReportsMissingBeforeFirstAndWithinPage(t *testing.T) {
	first := testKey(0x20)
	second := testKey(0x40)
	object := buildEncodingObject(t, encodingTestPage{
		directoryKey: first,
		body: encodingPage(t,
			encodingTestEntry{contentKey: first, decodedSize: 1, encodingKeys: [][16]byte{testKey(0xa1)}},
			encodingTestEntry{contentKey: second, decodedSize: 2, encodingKeys: [][16]byte{testKey(0xa2)}},
		),
	})
	index := openEncodingTestObject(t, object.raw)
	for name, key := range map[string]string{
		"before first": testKeyString(testKey(0x10)),
		"within page":  testKeyString(testKey(0x30)),
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := index.FindContent(context.Background(), key); !errors.Is(err, records.ErrContentMissing) {
				t.Fatalf("FindContent error = %v, want ErrContentMissing", err)
			}
		})
	}
}

func TestEncodingFindContentRejectsCorruptPageChecksum(t *testing.T) {
	contentKey := testKey(0x20)
	object := buildEncodingObject(t, encodingTestPage{
		directoryKey: contentKey,
		body: encodingPage(t, encodingTestEntry{
			contentKey:   contentKey,
			decodedSize:  1,
			encodingKeys: [][16]byte{testKey(0xa0)},
		}),
	})
	corruptLogical := append([]byte(nil), object.logical...)
	corruptLogical[object.pagesStart+100] ^= 1
	corrupt := frameEncoding(corruptLogical)
	index := openEncodingTestObject(t, corrupt)
	if got, err := index.FindContent(context.Background(), testKeyString(contentKey)); !errors.Is(err, container.ErrIntegrity) || got.ContentKey != "" || got.DecodedBytes != 0 || len(got.EncodingKeys) != 0 || got.Page != 0 {
		t.Fatalf("FindContent = %#v, %v; want integrity error and no partial record", got, err)
	}
}

func TestOpenEncodingRejectsUnsortedDirectory(t *testing.T) {
	object := buildEncodingObject(t,
		encodingTestPage{directoryKey: testKey(0x20), body: emptyEncodingPage()},
		encodingTestPage{directoryKey: testKey(0x10), body: emptyEncodingPage()},
	)
	if _, err := openEncodingFromLogical(t, object.logical); !errors.Is(err, records.ErrMetadataFormat) {
		t.Fatalf("OpenEncoding error = %v, want ErrMetadataFormat", err)
	}
}

func TestEncodingFindContentRejectsPageKeyFirstMismatch(t *testing.T) {
	directoryKey := testKey(0x10)
	pageKey := testKey(0x20)
	object := buildEncodingObject(t, encodingTestPage{
		directoryKey: directoryKey,
		body: encodingPage(t, encodingTestEntry{
			contentKey:   pageKey,
			decodedSize:  1,
			encodingKeys: [][16]byte{testKey(0xa0)},
		}),
	})
	index := openEncodingTestObject(t, object.raw)
	if _, err := index.FindContent(context.Background(), testKeyString(pageKey)); !errors.Is(err, records.ErrMetadataFormat) {
		t.Fatalf("FindContent error = %v, want ErrMetadataFormat", err)
	}
}

func TestEncodingFindContentRejectsPageKeyNextBoundaryMismatch(t *testing.T) {
	first := testKey(0x10)
	next := testKey(0x20)
	pastBoundary := testKey(0x30)
	object := buildEncodingObject(t,
		encodingTestPage{
			directoryKey: first,
			body: encodingPage(t,
				encodingTestEntry{contentKey: first, decodedSize: 1, encodingKeys: [][16]byte{testKey(0xa0)}},
				encodingTestEntry{contentKey: pastBoundary, decodedSize: 2, encodingKeys: [][16]byte{testKey(0xb0)}},
			),
		},
		encodingTestPage{directoryKey: next, body: emptyEncodingPage()},
	)
	index := openEncodingTestObject(t, object.raw)
	if _, err := index.FindContent(context.Background(), testKeyString(first)); !errors.Is(err, records.ErrMetadataFormat) {
		t.Fatalf("FindContent error = %v, want ErrMetadataFormat", err)
	}
}

func TestEncodingFindContentValidatesTheFullCandidatePage(t *testing.T) {
	contentKey := testKey(0x10)
	page := encodingPage(t, encodingTestEntry{
		contentKey:   contentKey,
		decodedSize:  1,
		encodingKeys: [][16]byte{testKey(0xa0)},
	})
	page[22+16] = 64
	object := buildEncodingObject(t, encodingTestPage{directoryKey: contentKey, body: page})
	index := openEncodingTestObject(t, object.raw)
	if _, err := index.FindContent(context.Background(), testKeyString(contentKey)); !errors.Is(err, records.ErrMetadataFormat) {
		t.Fatalf("FindContent error = %v, want ErrMetadataFormat", err)
	}
}

func TestEncodingFindContentRejectsTruncatedEntry(t *testing.T) {
	contentKey := testKey(0x10)
	page := emptyEncodingPage()
	page[0] = 64
	object := buildEncodingObject(t, encodingTestPage{directoryKey: contentKey, body: page})
	index := openEncodingTestObject(t, object.raw)
	if _, err := index.FindContent(context.Background(), testKeyString(contentKey)); !errors.Is(err, records.ErrMetadataFormat) {
		t.Fatalf("FindContent error = %v, want ErrMetadataFormat", err)
	}
}

func TestOpenEncodingRejectsWrongVersionKeyWidthsAndDeclaredBounds(t *testing.T) {
	object := buildEncodingObject(t, encodingTestPage{directoryKey: testKey(0x10), body: emptyEncodingPage()})
	cases := []struct {
		name   string
		mutate func([]byte)
	}{
		{name: "wrong version", mutate: func(data []byte) { data[2] = 2 }},
		{name: "wrong CKey width", mutate: func(data []byte) { data[3] = 15 }},
		{name: "wrong EKey width", mutate: func(data []byte) { data[4] = 15 }},
		{name: "declared bounds", mutate: func(data []byte) { binary.BigEndian.PutUint32(data[9:13], 2) }},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			logical := append([]byte(nil), object.logical...)
			testCase.mutate(logical)
			if _, err := openEncodingFromLogical(t, logical); !errors.Is(err, records.ErrMetadataFormat) {
				t.Fatalf("OpenEncoding error = %v, want ErrMetadataFormat", err)
			}
		})
	}
}

func TestOpenEncodingAcceptsEmptyCKeyDirectory(t *testing.T) {
	object := buildEncodingObject(t)
	if index := openEncodingTestObject(t, object.raw); index == nil {
		t.Fatal("OpenEncoding returned a nil index")
	}
}

func TestOpenEncodingRejectsZeroPageHash(t *testing.T) {
	object := buildEncodingObject(t, encodingTestPage{directoryKey: testKey(0x10), body: emptyEncodingPage()})
	logical := append([]byte(nil), object.logical...)
	for index := 22 + 16; index < 22+32; index++ {
		logical[index] = 0
	}
	if _, err := openEncodingFromLogical(t, logical); !errors.Is(err, container.ErrIntegrity) {
		t.Fatalf("OpenEncoding error = %v, want container.ErrIntegrity", err)
	}
}

func TestEncodingHonorsCancellation(t *testing.T) {
	contentKey := testKey(0x10)
	object := buildEncodingObject(t, encodingTestPage{
		directoryKey: contentKey,
		body: encodingPage(t, encodingTestEntry{
			contentKey:   contentKey,
			decodedSize:  1,
			encodingKeys: [][16]byte{testKey(0xa0)},
		}),
	})
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(object.raw), int64(len(object.raw)), encodingTestLimits, nil)
	if err != nil {
		t.Fatalf("OpenRanges: %v", err)
	}
	index, err := records.OpenEncoding(context.Background(), ranges)
	if err != nil {
		t.Fatalf("OpenEncoding: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := index.FindContent(ctx, testKeyString(contentKey)); !errors.Is(err, context.Canceled) {
		t.Fatalf("FindContent error = %v, want context.Canceled", err)
	}

	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := records.OpenEncoding(cancelled, ranges); !errors.Is(err, context.Canceled) {
		t.Fatalf("OpenEncoding error = %v, want context.Canceled", err)
	}
}
