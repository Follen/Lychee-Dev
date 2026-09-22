package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"testing"
)

func cdnIndexFixture(width int, keys []string, sizes []uint32, offsets []uint64) ([]byte, string) {
	const pageSize = 4096
	stride := 20 + width
	perPage := pageSize / stride
	pages := (len(keys) + perPage - 1) / perPage
	raw := make([]byte, pages*(pageSize+24)+28)
	for i, key := range keys {
		p, slot := i/perPage, i%perPage
		pos := p*pageSize + slot*stride
		decoded, _ := hex.DecodeString(key)
		copy(raw[pos:], decoded)
		binary.BigEndian.PutUint32(raw[pos+16:], sizes[i])
		for j := 0; j < width; j++ {
			raw[pos+20+j] = byte(offsets[i] >> uint((width-j-1)*8))
		}
		copy(raw[pages*pageSize+p*16:], decoded)
	}
	for p := 0; p < pages; p++ {
		sum := md5.Sum(raw[p*pageSize : (p+1)*pageSize])
		copy(raw[pages*(pageSize+16)+p*8:], sum[:8])
	}
	foot := raw[len(raw)-28:]
	toc := md5.Sum(raw[pages*pageSize : len(raw)-28])
	copy(foot, toc[:8])
	foot[8], foot[11], foot[12], foot[13], foot[14], foot[15] = 1, 4, byte(width), 4, 16, 8
	binary.LittleEndian.PutUint32(foot[16:], uint32(len(keys)))
	check := make([]byte, 20)
	copy(check, foot[8:20])
	sum := md5.Sum(check)
	copy(foot[20:], sum[:8])
	sum = md5.Sum(foot)
	return raw, hex.EncodeToString(sum[:])
}

func TestCDNIndexAuthenticatedSelection(t *testing.T) {
	for _, width := range []int{0, 4, 5, 6} {
		t.Run(fmt.Sprint(width), func(t *testing.T) {
			keys := []string{}
			sizes := []uint32{}
			offsets := []uint64{}
			for i := 0; i < 400; i++ {
				keys = append(keys, fmt.Sprintf("%032x", i+1))
				sizes = append(sizes, 123)
				offset := uint64(456)
				if width > 4 {
					offset |= uint64(1) << 32
				}
				offsets = append(offsets, offset)
			}
			raw, key := cdnIndexFixture(width, keys, sizes, offsets)
			index, err := openCDNIndex(context.Background(), bytes.NewReader(raw), int64(len(raw)), key)
			if err != nil {
				t.Fatal(err)
			}
			for _, i := range []int{0, 170, 399} {
				span, err := index.locate(context.Background(), keys[i], []string{keys[0], keys[1]}, keys[2])
				if err != nil {
					t.Fatal(err)
				}
				want := cdnSpan{Object: keys[2], Bytes: 123, Offset: 456}
				if width == 0 {
					want.Object = keys[i]
					want.Offset = 0
				}
				if width > 4 {
					want.Object = keys[1]
				}
				if span != want {
					t.Fatal(span, want)
				}
			}
			if _, err := index.locate(context.Background(), fmt.Sprintf("%032x", 9999), nil, keys[2]); !errors.Is(err, ErrContentMissing) {
				t.Fatal(err)
			}
		})
	}
}

func TestCDNIndexRejectsCorruptMetadataAndPage(t *testing.T) {
	wanted := fmt.Sprintf("%032x", 1)
	raw, key := cdnIndexFixture(4, []string{wanted}, []uint32{123}, []uint64{456})
	for _, position := range []int{len(raw) - 1, 4096, len(raw) - 28} {
		bad := bytes.Clone(raw)
		bad[position] ^= 1
		if _, err := openCDNIndex(context.Background(), bytes.NewReader(bad), int64(len(bad)), key); !errors.Is(err, ErrConfigurationIntegrity) {
			t.Fatal(position, err)
		}
	}
	bad := bytes.Clone(raw)
	bad[0] ^= 1
	index, err := openCDNIndex(context.Background(), bytes.NewReader(bad), int64(len(bad)), key)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := index.locate(context.Background(), wanted, nil, key); !errors.Is(err, ErrConfigurationIntegrity) {
		t.Fatal(err)
	}
	if _, err := openCDNIndex(context.Background(), bytes.NewReader(raw), int64(len(raw)-1), key); err == nil {
		t.Fatal("truncation accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := openCDNIndex(ctx, bytes.NewReader(raw), int64(len(raw)), key); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func FuzzCDNIndex(f *testing.F) {
	raw, key := cdnIndexFixture(4, []string{fmt.Sprintf("%032x", 1)}, []uint32{123}, []uint64{0})
	f.Add(raw, key)
	f.Fuzz(func(t *testing.T, raw []byte, key string) {
		if len(raw) > 1<<20 {
			return
		}
		index, err := openCDNIndex(context.Background(), bytes.NewReader(raw), int64(len(raw)), key)
		if err == nil {
			_, _ = index.locate(context.Background(), fmt.Sprintf("%032x", 1), nil, key)
		}
		// Also cross the content-address admission check: mutated format bytes
		// should exercise structural validation, not only a fixed MD5 mismatch.
		if len(raw) >= 28 {
			sum := md5.Sum(raw[len(raw)-28:])
			derived := hex.EncodeToString(sum[:])
			index, err = openCDNIndex(context.Background(), bytes.NewReader(raw), int64(len(raw)), derived)
			if err == nil {
				_, _ = index.locate(context.Background(), fmt.Sprintf("%032x", 1), nil, derived)
			}
		}
	})
}

func TestCDNIndexRejectsAuthenticatedInvalidEntries(t *testing.T) {
	first, second := fmt.Sprintf("%032x", 1), fmt.Sprintf("%032x", 2)
	for _, scenario := range []string{"duplicate", "zero-key", "short-object", "ordinal", "padding", "toc-last"} {
		t.Run(scenario, func(t *testing.T) {
			raw, _ := cdnIndexFixture(6, []string{first, second}, []uint32{123, 123}, []uint64{456, 789})
			switch scenario {
			case "duplicate":
				copy(raw[26:42], raw[:16])
			case "zero-key":
				clear(raw[:16])
			case "short-object":
				binary.BigEndian.PutUint32(raw[16:20], 7)
			case "ordinal":
				binary.BigEndian.PutUint16(raw[20:22], 1)
			case "padding":
				raw[52] = 1
			case "toc-last":
				raw[4096+15] = 3
			}
			// Authenticate the deliberately invalid writer output. Rejection
			// must come from structure checks rather than a stale test checksum.
			sum := md5.Sum(raw[:4096])
			copy(raw[4112:4120], sum[:8])
			sum = md5.Sum(raw[4096:4120])
			copy(raw[4120:4128], sum[:8])
			sum = md5.Sum(raw[4120:])
			key := hex.EncodeToString(sum[:])
			index, err := openCDNIndex(context.Background(), bytes.NewReader(raw), int64(len(raw)), key)
			if err != nil {
				t.Fatalf("metadata should authenticate before page validation: %v", err)
			}
			if _, err := index.locate(context.Background(), first, []string{key}, key); !errors.Is(err, ErrMetadataFormat) {
				t.Fatalf("accepted invalid %s: %v", scenario, err)
			}
		})
	}
}
