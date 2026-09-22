package container_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

func TestCheckedSpanOnlyReadsLogicalRange(t *testing.T) {
	block := plainBlock("abcdefghij")
	raw := framedBLTE(block)
	source := newSynchronizedReaderAt(raw)
	limits := generousLimits
	limits.ChunkBytes = 4
	ranges, err := container.OpenRanges(context.Background(), source, int64(len(raw)), limits, nil)
	if err != nil {
		t.Fatal(err)
	}
	source.resetCalls()
	got, err := ranges.ReadCheckedSpan(context.Background(), 3, 3, md5.Sum([]byte("def")))
	if err != nil || string(got) != "def" {
		t.Fatalf("%q %v", got, err)
	}
	start := blockPayloadStart(raw)
	assertCalls(t, source, readerAtCall{offset: start, length: 1}, readerAtCall{offset: start + 4, length: 3})
	if got, err := ranges.ReadCheckedSpan(context.Background(), 3, 3, md5.Sum([]byte("bad"))); !errors.Is(err, container.ErrIntegrity) || got != nil {
		t.Fatalf("bad logical hash accepted: %q %v", got, err)
	}
}

func TestCheckedSpanFallbackAndScope(t *testing.T) {
	ctx := context.Background()
	for _, blocks := range [][]fixtureBlock{{zlibBlock(t, "abcdefgh")}, {plainBlock("abc"), plainBlock("defgh")}} {
		raw := framedBLTE(blocks...)
		ranges, err := container.OpenRanges(ctx, bytes.NewReader(raw), int64(len(raw)), generousLimits, nil)
		if err != nil {
			t.Fatal(err)
		}
		got, err := ranges.ReadCheckedSpan(ctx, 2, 4, md5.Sum([]byte("cdef")))
		if err != nil || string(got) != "cdef" {
			t.Fatalf("%q %v", got, err)
		}
	}
	raw := framedBLTE(plainBlock("abcdefgh"))
	raw[len(raw)-1] ^= 1 // Outside the logical range; the whole block is corrupt.
	ranges, err := container.OpenRanges(ctx, bytes.NewReader(raw), int64(len(raw)), generousLimits, nil)
	if err != nil {
		t.Fatal(err)
	}
	if got, err := ranges.ReadCheckedSpan(ctx, 0, 3, md5.Sum([]byte("abc"))); err != nil || string(got) != "abc" {
		t.Fatalf("logical range: %q %v", got, err)
	}
	if _, err := ranges.ReadSpan(ctx, 0, 3); !errors.Is(err, container.ErrIntegrity) {
		t.Fatal("ordinary range did not check whole block", err)
	}
	if _, err := ranges.ReadCheckedSpan(ctx, 0, 3, [16]byte{}); !errors.Is(err, container.ErrIntegrity) {
		t.Fatal("zero digest accepted", err)
	}
}

func TestOrdinaryRangeRejectsAbsentEmbeddedChecksum(t *testing.T) {
	block := plainBlock("abc")
	block.checksum = make([]byte, 16)
	raw := framedBLTE(block)
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(raw), int64(len(raw)), generousLimits, nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ranges.ReadSpan(context.Background(), 0, 3); !errors.Is(err, container.ErrIntegrity) {
		t.Fatalf("missing checksum accepted: %v", err)
	}
	if got, err := ranges.ReadCheckedSpan(context.Background(), 0, 3, md5.Sum([]byte("abc"))); err != nil || string(got) != "abc" {
		t.Fatalf("external logical checksum: %q %v", got, err)
	}
}
