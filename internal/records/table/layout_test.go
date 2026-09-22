package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/table"
)

var limits = table.Budget{FileBytes: 1 << 20, MetadataBytes: 1 << 16, Rows: 1000, Columns: 100, Partitions: 100}

func fixture(version int) ([]byte, int, int) {
	headerSize, sectionSize := 72, 40
	if version == 5 {
		headerSize += 132
	}
	if version == 2 {
		sectionSize = 36
	}
	start := headerSize + sectionSize + 4 + 24
	raw := make([]byte, start+8)
	copy(raw, fmt.Sprintf("WDC%d", version))
	base := 4
	if version == 5 {
		binary.LittleEndian.PutUint32(raw[4:8], 1)
		copy(raw[8:136], "12.1.0.69875")
		base += 132
	}
	w := func(offset int, v uint32) { binary.LittleEndian.PutUint32(raw[base+offset:base+offset+4], v) }
	w(0, 2)
	w(4, 1)
	w(8, 4)
	w(16, 0xaabbccdd)
	w(20, 0x12345678)
	w(24, 1)
	w(28, 2)
	w(40, 1)
	w(52, 24)
	w(64, 1)
	binary.LittleEndian.PutUint32(raw[headerSize+8:headerSize+12], uint32(start))
	binary.LittleEndian.PutUint32(raw[headerSize+12:headerSize+16], 2)
	field := headerSize + sectionSize + 4
	binary.LittleEndian.PutUint16(raw[field+2:field+4], 32)
	binary.LittleEndian.PutUint32(raw[start:start+4], 1)
	binary.LittleEndian.PutUint32(raw[start+4:start+8], 2)
	return raw, base, start
}

func TestInspectVersionsAndExactHashes(t *testing.T) {
	for _, version := range []int{2, 3, 4, 5} {
		raw, _, start := fixture(version)
		got, err := table.Inspect(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			t.Fatalf("v%d: %v", version, err)
		}
		if got.Version != version || got.Rows != 2 || got.TableHash != 0xaabbccdd || got.LayoutHash != 0x12345678 || got.MetadataEnd != int64(start) || len(got.Fields) != 1 || got.Fields[0].BitWidth != 32 || len(got.Placements) != 1 {
			t.Fatalf("%+v", got)
		}
		if version == 5 && (got.SchemaRevision != 1 || string(bytes.TrimRight(got.SchemaBuild[:], "\x00")) != "12.1.0.69875") {
			t.Fatal("lost schema identity")
		}
	}
}

func TestInspectRejectsCorruptionAndBudgets(t *testing.T) {
	for _, name := range []string{"truncated", "rows", "storage", "palette", "common", "offset", "overrun", "id-bytes", "codec", "partition-count", "column-count"} {
		t.Run(name, func(t *testing.T) {
			raw, base, start := fixture(3)
			put := func(offset int, v uint32) { binary.LittleEndian.PutUint32(raw[offset:offset+4], v) }
			switch name {
			case "truncated":
				raw = raw[:start-1]
			case "rows":
				put(base, 3)
			case "storage":
				put(base+52, 25)
			case "palette":
				put(base+60, 4)
			case "common":
				put(base+56, 8)
			case "offset":
				put(80, 1)
			case "overrun":
				put(80, uint32(len(raw)))
			case "id-bytes":
				put(96, 1)
			case "codec":
				put(start-16, 6)
			case "partition-count":
				put(base+64, 10001)
			case "column-count":
				put(base+40, 10001)
			}
			if got, err := table.Inspect(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits); err == nil || got.Rows != 0 {
				t.Fatalf("accepted %s: %+v %v", name, got, err)
			}
		})
	}
	raw, _, _ := fixture(3)
	for _, name := range []string{"file", "metadata", "rows"} {
		budget := limits
		switch name {
		case "file":
			budget.FileBytes = 4
		case "metadata":
			budget.MetadataBytes = 10
		case "rows":
			budget.Rows = 1
		}
		if _, err := table.Inspect(context.Background(), bytes.NewReader(raw), int64(len(raw)), budget); !errors.Is(err, table.ErrLimit) {
			t.Fatalf("%s: %v", name, err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), limits); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

type tracedReader struct {
	raw []byte
	end int64
}

func (r *tracedReader) ReadAt(dst []byte, offset int64) (int, error) {
	if offset+int64(len(dst)) > r.end {
		r.end = offset + int64(len(dst))
	}
	return bytes.NewReader(r.raw).ReadAt(dst, offset)
}
func TestInspectDoesNotReadRows(t *testing.T) {
	raw, _, start := fixture(3)
	source := &tracedReader{raw: raw}
	if _, err := table.Inspect(context.Background(), source, int64(len(raw)), limits); err != nil {
		t.Fatal(err)
	}
	if source.end > int64(start) {
		t.Fatal("metadata inspection read row payload")
	}
}

func TestEncryptedIdentityListsFollowKeyedPartitions(t *testing.T) {
	for _, version := range []int{4, 5} {
		raw, _, start := fixture(version)
		section := 72
		if version == 5 {
			section += 132
		}
		binary.LittleEndian.PutUint64(raw[section:section+8], 7)
		binary.LittleEndian.PutUint32(raw[section+8:section+12], uint32(start+12))
		withIDs := append([]byte{}, raw[:start]...)
		withIDs = append(withIDs, words(2, 1, 2)...)
		withIDs = append(withIDs, raw[start:]...)
		got, err := table.Inspect(context.Background(), bytes.NewReader(withIDs), int64(len(withIDs)), limits)
		if err != nil || got.MetadataEnd != int64(start+12) {
			t.Fatalf("v%d: %+v %v", version, got, err)
		}
	}
	base, _, _ := fixture(4)
	raw := make([]byte, 188)
	copy(raw[:72], base[:72])
	binary.LittleEndian.PutUint32(raw[68:72], 2)
	for i := 0; i < 2; i++ {
		section := 72 + i*40
		binary.LittleEndian.PutUint32(raw[section+8:section+12], uint32(180+i*4))
		binary.LittleEndian.PutUint32(raw[section+12:section+16], 1)
	}
	copy(raw[156:180], base[116:140])
	copy(raw[180:], words(1, 2))
	got, err := table.Inspect(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil || got.MetadataEnd != 180 {
		t.Fatalf("non-keyed partitions: %+v %v", got, err)
	}
}

func FuzzInspect(f *testing.F) {
	for _, version := range []int{2, 3, 4, 5} {
		raw, _, _ := fixture(version)
		f.Add(raw)
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 1<<20 {
			t.Skip()
		}
		_, _ = table.Inspect(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	})
}
