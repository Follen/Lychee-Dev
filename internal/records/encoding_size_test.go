package records_test

import (
	"context"
	"crypto/md5"
	"encoding/binary"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
)

func encodedFixture(t *testing.T) []byte {
	t.Helper()
	logical := buildEncodingObject(t).logical
	binary.BigEndian.PutUint32(logical[13:17], 2)
	logical = append(logical, make([]byte, 64+2048)...)
	for i, b := range []byte{0x20, 0x80} {
		key := testKey(b)
		copy(logical[22+i*32:], key[:])
		page := logical[86+i*1024 : 86+(i+1)*1024]
		copy(page, key[:])
		binary.BigEndian.PutUint32(page[16:20], uint32(i+7))
		putUint40(page[20:25], (1<<33)+uint64(i))
		digest := md5.Sum(page)
		copy(logical[22+i*32+16:], digest[:])
	}
	return logical
}

func TestEncodingPhysicalSize(t *testing.T) {
	index, err := openEncodingFromLogical(t, encodedFixture(t))
	if err != nil {
		t.Fatal(err)
	}
	for i, b := range []byte{0x20, 0x80} {
		record, err := index.FindEncoding(context.Background(), testKeyString(testKey(b)))
		if err != nil || record.EncodedBytes != (1<<33)+int64(i) || record.Specification != uint32(i+7) {
			t.Fatalf("record=%+v err=%v", record, err)
		}
	}
	for _, b := range []byte{0x10, 0x30, 0x90} {
		if _, err := index.FindEncoding(context.Background(), testKeyString(testKey(b))); !errors.Is(err, records.ErrEncodingMissing) {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := index.FindEncoding(ctx, testKeyString(testKey(0x20))); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestEncodingPhysicalPageRejectsDamage(t *testing.T) {
	for _, name := range []string{"checksum", "first", "order", "next", "size", "padding", "empty", "zero-hash", "directory-order"} {
		t.Run(name, func(t *testing.T) {
			logical := encodedFixture(t)
			page := logical[86:1110]
			switch name {
			case "checksum":
				page[24] ^= 1
			case "first":
				page[0]++
			case "order":
				copy(page[25:50], page[:25])
			case "next":
				copy(page[25:50], page[:25])
				key := testKey(0x80)
				copy(page[25:41], key[:])
			case "size":
				clear(page[20:25])
			case "padding":
				page[len(page)-1] = 1
			case "empty":
				clear(page)
			case "zero-hash":
				clear(logical[38:54])
			case "directory-order":
				copy(logical[54:70], logical[22:38])
			}
			if name != "checksum" && name != "zero-hash" {
				digest := md5.Sum(page)
				copy(logical[38:54], digest[:])
			}
			index, err := openEncodingFromLogical(t, logical)
			if err != nil {
				t.Fatal(err)
			}
			record, err := index.FindEncoding(context.Background(), testKeyString(testKey(0x20)))
			want := records.ErrMetadataFormat
			if name == "checksum" || name == "zero-hash" {
				want = container.ErrIntegrity
			}
			if !errors.Is(err, want) || record != (records.EncodedRecord{}) {
				t.Fatalf("record=%+v err=%v want=%v", record, err, want)
			}
		})
	}
}

func TestEncodingPhysicalDirectoryBounds(t *testing.T) {
	for _, name := range []string{"truncated", "zero-size", "huge-count"} {
		t.Run(name, func(t *testing.T) {
			logical := encodedFixture(t)
			switch name {
			case "truncated":
				logical = logical[:len(logical)-1]
			case "zero-size":
				clear(logical[7:9])
			case "huge-count":
				binary.BigEndian.PutUint32(logical[13:17], 1<<21)
			}
			if _, err := openEncodingFromLogical(t, logical); err == nil {
				t.Fatal("accepted invalid EKey extent")
			}
		})
	}
}

func FuzzEncodingPhysicalPage(f *testing.F) {
	f.Add([]byte{0x20})
	f.Add(make([]byte, 1024))
	f.Fuzz(func(t *testing.T, data []byte) {
		if len(data) > 1024 {
			t.Skip()
		}
		logical := encodedFixture(t)
		page := logical[86:1110]
		clear(page)
		copy(page, data)
		digest := md5.Sum(page)
		copy(logical[38:54], digest[:])
		index, err := openEncodingFromLogical(t, logical)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = index.FindEncoding(context.Background(), testKeyString(testKey(0x20)))
	})
}
