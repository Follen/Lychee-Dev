package container_test

import (
	"bytes"
	"compress/zlib"
	"context"
	"encoding/binary"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

func TestCompressedExpansionAndExactBudgets(t *testing.T) {
	var compressed bytes.Buffer
	zw := zlib.NewWriter(&compressed)
	_, _ = zw.Write(bytes.Repeat([]byte{'a'}, 1<<20))
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	raw := append([]byte("BLTE\x00\x00\x00\x00Z"), compressed.Bytes()...)
	limits := container.Limits{EncodedBytes: int64(len(raw)), DecodedBytes: 4096, ChunkBytes: 4096, Chunks: 32, Depth: 4}
	var result bytes.Buffer
	if _, err := container.Decode(context.Background(), &result, bytes.NewReader(raw), limits); !errors.Is(err, container.ErrLimit) || result.Len() != 0 {
		t.Fatalf("expansion accepted: %d %v", result.Len(), err)
	}
	plain := []byte("BLTE\x00\x00\x00\x00Nabcd")
	limits.EncodedBytes, limits.DecodedBytes, limits.ChunkBytes = int64(len(plain)), 4, 5
	if n, err := container.Decode(context.Background(), &result, bytes.NewReader(plain), limits); err != nil || n != 4 || result.String() != "abcd" {
		t.Fatalf("exact budgets rejected: %d %q %v", n, result.String(), err)
	}
}

func TestCompressedTrailingDataAndHostileChunkCount(t *testing.T) {
	var compressed bytes.Buffer
	zw := zlib.NewWriter(&compressed)
	_, _ = zw.Write([]byte("hello"))
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	data := append([]byte("BLTE\x00\x00\x00\x00Z"), compressed.Bytes()...)
	data = append(data, 1)
	var result bytes.Buffer
	limits := container.Limits{EncodedBytes: 4096, DecodedBytes: 4096, ChunkBytes: 4096, Chunks: 32, Depth: 4}
	if _, err := container.Decode(context.Background(), &result, bytes.NewReader(data), limits); !errors.Is(err, container.ErrMalformed) || result.Len() != 0 {
		t.Fatalf("zlib trailing data: %q %v", result.String(), err)
	}
	header := []byte("BLTE\x00\x00\x00\x00\x0f\xff\xff\xff")
	binary.BigEndian.PutUint32(header[4:8], 12+0xffffff*24)
	if _, err := container.Decode(context.Background(), &result, bytes.NewReader(header), limits); !errors.Is(err, container.ErrLimit) {
		t.Fatalf("unbounded header allocation not rejected: %v", err)
	}
}

func TestRangeBudgetAppliesToVisitedChunks(t *testing.T) {
	data := framedBLTE(plainBlock("ok"), plainBlock("oversized"))
	limits := container.Limits{EncodedBytes: 4096, DecodedBytes: 4096, ChunkBytes: 4, Chunks: 8, Depth: 4}
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(data), int64(len(data)), limits, nil)
	if err != nil {
		t.Fatal(err)
	}
	got, err := ranges.ReadSpan(context.Background(), 0, 2)
	if err != nil || string(got) != "ok" {
		t.Fatalf("small metadata blocked: %q %v", got, err)
	}
	got, err = ranges.ReadSpan(context.Background(), 2, 1)
	if !errors.Is(err, container.ErrLimit) || got != nil {
		t.Fatalf("oversized visited chunk accepted: %q %v", got, err)
	}
}
