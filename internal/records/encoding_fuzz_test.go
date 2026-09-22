package records_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
)

func FuzzEncodingPage(f *testing.F) {
	seed := make([]byte, 38)
	seed[0] = 1
	seed[6] = 1
	seed[22] = 2
	f.Add(seed)
	f.Fuzz(func(t *testing.T, input []byte) {
		page := make([]byte, 1024)
		copy(page, input)
		payload := make([]byte, 54+len(page))
		copy(payload, []byte{'E', 'N', 1, 16, 16, 0, 1, 0, 1, 0, 0, 0, 1})
		payload[22] = 1
		sum := md5.Sum(page)
		copy(payload[38:54], sum[:])
		copy(payload[54:], page)
		encoded := append([]byte{'N'}, payload...)
		blte := make([]byte, 36+len(encoded))
		copy(blte, "BLTE")
		binary.BigEndian.PutUint32(blte[4:8], 36)
		blte[8] = 15
		blte[11] = 1
		binary.BigEndian.PutUint32(blte[12:16], uint32(len(encoded)))
		binary.BigEndian.PutUint32(blte[16:20], uint32(len(payload)))
		blockHash := md5.Sum(encoded)
		copy(blte[20:36], blockHash[:])
		copy(blte[36:], encoded)
		ctx := context.Background()
		ranges, err := container.OpenRanges(ctx, bytes.NewReader(blte), int64(len(blte)), container.Limits{EncodedBytes: 8192, DecodedBytes: 8192, ChunkBytes: 8192, Chunks: 8, Depth: 4}, nil)
		if err != nil {
			t.Fatal(err)
		}
		index, err := records.OpenEncoding(ctx, ranges)
		if err != nil {
			t.Fatal(err)
		}
		record, err := index.FindContent(ctx, "01000000000000000000000000000000")
		if err != nil && record.ContentKey != "" {
			t.Fatal("partial record on failure")
		}
		if err == nil && (record.ContentKey != "01000000000000000000000000000000" || len(record.EncodingKeys) == 0 || len(record.EncodingKeys) > 63 || record.DecodedBytes < 0) {
			t.Fatalf("invalid result %+v", record)
		}
	})
}
