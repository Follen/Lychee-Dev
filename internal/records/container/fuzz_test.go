package container_test

import (
	"bytes"
	"context"
	"encoding/hex"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

func FuzzContainerBudgets(f *testing.F) {
	f.Add([]byte("BLTE\x00\x00\x00\x00Nhello"))
	f.Add([]byte("BLTE\x00\x00\x00\x00FBLTE\x00\x00\x00\x00N"))
	f.Add([]byte("BLTE\x00\x00\x00\x24\x0f\x00\x00\x01"))
	f.Fuzz(func(t *testing.T, data []byte) {
		var output bytes.Buffer
		n, err := container.Decode(context.Background(), &output, bytes.NewReader(data), container.Limits{
			EncodedBytes: 4096, DecodedBytes: 1024, ChunkBytes: 512, Chunks: 32, Depth: 4,
		})
		if n != int64(output.Len()) || n > 1024 {
			t.Fatalf("output budget/count violated: n=%d len=%d err=%v", n, output.Len(), err)
		}
	})
}

func FuzzKeyedContainerBudgets(f *testing.F) {
	ciphertext, err := hex.DecodeString("63a3a685d34d49920c2224620bf4f1f92ebda59d3ec7b0dd2f9aff")
	if err != nil {
		f.Fatal(err)
	}
	f.Add(append([]byte("BLTE\x00\x00\x00\x00E\x08\x01\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00\x00S"), ciphertext...))
	f.Fuzz(func(t *testing.T, data []byte) {
		var output bytes.Buffer
		key := make([]byte, 16)
		for i := range key {
			key[i] = byte(i)
		}
		n, err := container.DecodeWithKeys(context.Background(), &output, bytes.NewReader(data), container.Limits{
			EncodedBytes: 4096, DecodedBytes: 1024, ChunkBytes: 512, Chunks: 32, Depth: 4,
		}, func(context.Context, uint64) ([]byte, error) { return key, nil })
		if n != int64(output.Len()) || n > 1024 {
			t.Fatalf("output budget/count: %d %d %v", n, output.Len(), err)
		}
	})
}

func FuzzRangeDirectory(f *testing.F) {
	f.Add([]byte("BLTE\x00\x00\x00\x24\x0f\x00\x00\x01\x00\x00\x00\x02\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00Na"), int64(0), int64(1))
	f.Fuzz(func(t *testing.T, data []byte, offset, length int64) {
		ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(data), int64(len(data)), container.Limits{
			EncodedBytes: 4096, DecodedBytes: 1024, ChunkBytes: 512, Chunks: 32, Depth: 4,
		}, nil)
		if err != nil {
			return
		}
		result, err := ranges.ReadSpan(context.Background(), offset, length)
		if err != nil && result != nil {
			t.Fatal("partial result on failure")
		}
		if err == nil && (int64(len(result)) != length || len(result) > 512) {
			t.Fatalf("range budget: %d length %d", len(result), length)
		}
	})
}
