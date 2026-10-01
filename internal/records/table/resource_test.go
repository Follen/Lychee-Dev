package table_test

import (
	"bytes"
	"context"
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
