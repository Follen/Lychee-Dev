package table_test

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/table"
)

func TestScanOrderedCopiesAndBounds(t *testing.T) {
	v := pageView(t)
	ctx := context.Background()
	var ids []uint64
	err := v.Scan(ctx, 1024, 1<<20, func(row map[string]any) error { ids = append(ids, row["ID"].(uint64)); return nil })
	if err != nil {
		t.Fatal(err)
	}
	page, err := v.Page(ctx, nil, 200, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	want := []uint64{}
	for _, id := range page.IDs {
		want = append(want, uint64(id))
	}
	if !reflect.DeepEqual(ids, want) {
		t.Fatal(ids, want)
	}
	failure := errors.New("stop")
	count := 0
	err = v.Scan(ctx, 1024, 1<<20, func(map[string]any) error { count++; return failure })
	if !errors.Is(err, failure) || count != 1 {
		t.Fatal(count, err)
	}
	if err := v.Scan(ctx, 0, 1<<20, func(map[string]any) error { return nil }); !errors.Is(err, table.ErrLimit) {
		t.Fatal(err)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if err := v.Scan(cancelled, 1024, 1<<20, func(map[string]any) error { return nil }); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
