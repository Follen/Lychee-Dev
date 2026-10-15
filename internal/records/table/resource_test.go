package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/records/table"
)

func TestQueryBudgetSharedAcrossPreparedTablesAndRows(t *testing.T) {
	raw, _, _ := fixture(3)
	limits := limits
	caps := resource.DefaultLimits()
	caps.DecodeWork = 19
	budget := resource.New(caps)
	limits.Query = budget
	rows, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	view, err := rows.Bind(context.Background(), definition(t, "int ID", "$id$ID<32>"), "12.1.0.69875")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = view.Row(context.Background(), 1, 100); err != nil {
		t.Fatal(err)
	}
	if _, err = table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits); !errors.Is(err, resource.ErrBudget) {
		t.Fatalf("second table reset query work: %v", err)
	}
}

func TestQueryBudgetRejectsIndexBeforeReadingRecords(t *testing.T) {
	raw, _, start := fixture(3)
	limits := limits
	caps := resource.DefaultLimits()
	caps.MetadataBytes = 300
	limits.Query = resource.New(caps)
	source := &recordReadCounter{Reader: bytes.NewReader(raw), start: int64(start)}
	_, err := table.OpenRecords(context.Background(), source, int64(len(raw)), limits)
	if !errors.Is(err, resource.ErrBudget) || source.records != 0 {
		t.Fatalf("budget=%v record reads=%d", err, source.records)
	}
}

func TestCompactRecordIndexFitsLargeTableAfterSourcePreparation(t *testing.T) {
	// Retail zhCN 12.1.5.70391 SpellName has 189102 physical records and
	// 227231 copies. Shared CASC/definition preparation already reserves about
	// 93 MiB before its index opens. Exercise that shape with synthetic WDC5
	// bytes, including a lookup through the copy index with 48 MiB left for
	// the table. The former full-location map needed over 76 MiB for its index.
	const physicalRows, copyRows = 189102, 227231
	raw, base, start := fixture(5)
	raw = append(raw[:start], make([]byte, physicalRows*8+copyRows*8)...)
	put := func(offset int, value uint32) { binary.LittleEndian.PutUint32(raw[offset:offset+4], value) }
	put(base, physicalRows)
	put(base+28, physicalRows+copyRows)
	const partition = 204
	put(partition+12, physicalRows)
	put(partition+24, physicalRows*4)
	put(partition+36, copyRows)
	for i := 0; i < physicalRows; i++ {
		put(start+i*4, uint32(i+1))
		put(start+physicalRows*4+i*4, uint32(i+1))
	}
	for i := 0; i < copyRows; i++ {
		offset := start + physicalRows*8 + i*8
		put(offset, uint32(physicalRows+i+1))
		put(offset+4, uint32(i%physicalRows+1))
	}
	caps := resource.DefaultLimits()
	caps.MetadataBytes = 144 << 20
	query := resource.New(caps)
	if err := query.Charge(resource.Cost{RetainedBytes: 96 << 20, MetadataBytes: 96 << 20}); err != nil {
		t.Fatal(err)
	}
	budget := table.Budget{Query: query, FileBytes: 128 << 20, MetadataBytes: 64 << 20, Rows: 1000000, Columns: 4096, Partitions: 4096}
	rows, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	const copiedID = physicalRows + copyRows
	wantOrigin := uint32((copyRows-1)%physicalRows + 1)
	row, err := rows.Lookup(context.Background(), copiedID)
	if err != nil || row.ID != copiedID || row.OriginID != wantOrigin || len(row.Data) != 4 || binary.LittleEndian.Uint32(row.Data) != wantOrigin {
		t.Fatalf("copied row = %+v, error = %v; want origin %d", row, err, wantOrigin)
	}
}

func TestSparseAliasesShareLocationsAndConsumeQueryBudget(t *testing.T) {
	const aliases = 1000
	raw := sparseFixture(2, false)
	mapStart := int(binary.LittleEndian.Uint32(raw[96:100]))
	suffix := append([]byte(nil), raw[mapStart+24:]...)
	// Keep the explicit copy outside the expanded implicit-alias ID range.
	binary.LittleEndian.PutUint32(suffix[:4], 20000)
	alias := append([]byte(nil), raw[mapStart+18:mapStart+24]...)
	raw = raw[:mapStart+18]
	for range aliases {
		raw = append(raw, alias...)
	}
	raw = append(raw, suffix...)
	binary.LittleEndian.PutUint32(raw[32:36], 12+aliases)
	for _, small := range []bool{true, false} {
		caps := resource.DefaultLimits()
		if small {
			caps.MetadataBytes = 16 << 10
		}
		budget := limits
		budget.Rows = 1000000
		budget.Query = resource.New(caps)
		rows, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), budget)
		if small {
			if !errors.Is(err, resource.ErrBudget) || rows != nil {
				t.Fatalf("implicit aliases bypassed metadata cap: rows=%v, error=%v", rows != nil, err)
			}
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		row, err := rows.Lookup(context.Background(), 12+aliases)
		if err != nil || row.OriginID != 12 || !row.HasRelation || row.Relation != 777 {
			t.Fatalf("implicit alias lost its physical relationship: %+v, %v", row, err)
		}
	}
}

type recordReadCounter struct {
	*bytes.Reader
	start   int64
	records int
}

func (r *recordReadCounter) ReadAt(p []byte, off int64) (int, error) {
	if off >= r.start {
		r.records++
	}
	return r.Reader.ReadAt(p, off)
}
