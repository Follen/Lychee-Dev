package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/table"
)

func recordFixture(external bool, copies []byte, relation []byte) []byte {
	raw, _, _ := fixture(3)
	if external {
		binary.LittleEndian.PutUint32(raw[96:100], 8)
		raw = append(raw, words(10, 20)...)
	}
	binary.LittleEndian.PutUint32(raw[108:112], uint32(len(copies)/8))
	binary.LittleEndian.PutUint32(raw[100:104], uint32(len(relation)))
	raw = append(raw, copies...)
	return append(raw, relation...)
}

func TestRecordIdentityCopyChainsAndRelations(t *testing.T) {
	for _, external := range []bool{false, true} {
		id := uint32(1)
		if external {
			id = 10
		}
		raw := recordFixture(external, words(101, 100, 100, id), words(1, 0, 0, 0, 0))
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			t.Fatal(err)
		}
		for _, requested := range []uint32{id, 100, 101} {
			got, err := r.Lookup(context.Background(), requested)
			if err != nil || got.ID != requested || got.OriginID != id || !got.HasRelation || got.Relation != 0 || binary.LittleEndian.Uint32(got.Data) != 1 {
				t.Fatalf("%+v %v", got, err)
			}
			got.Data[0] = 99
			again, err := r.Lookup(context.Background(), requested)
			if err != nil || again.Data[0] != 1 {
				t.Fatal("row data alias")
			}
			values, err := r.Values(context.Background(), requested, 0, 1)
			want := uint64(requested)
			if external {
				want = 1
			}
			if err != nil || len(values) != 1 || values[0] != want {
				t.Fatalf("copy identity value: %v %v want %d", values, err, want)
			}
		}
		if _, err := r.Lookup(context.Background(), 999); !errors.Is(err, table.ErrRecordMissing) {
			t.Fatal(err)
		}
		var workers sync.WaitGroup
		for i := 0; i < 8; i++ {
			workers.Go(func() {
				for j := 0; j < 100; j++ {
					got, err := r.Lookup(context.Background(), 101)
					if err != nil || got.ID != 101 {
						t.Error("concurrent lookup failed")
					}
				}
			})
		}
		workers.Wait()
	}
}

func TestRecordIndexRejectsInvalidMetadata(t *testing.T) {
	for _, name := range []string{"cycle", "missing", "duplicate-copy", "physical-copy", "duplicate-id", "relation-ordinal", "duplicate-relation", "relation-size", "short-id", "sparse", "budget"} {
		t.Run(name, func(t *testing.T) {
			raw := recordFixture(true, nil, nil)
			budget := limits
			switch name {
			case "cycle":
				raw = recordFixture(true, words(100, 101, 101, 100), nil)
			case "missing":
				raw = recordFixture(true, words(100, 999), nil)
			case "duplicate-copy":
				raw = recordFixture(true, words(100, 10, 100, 20), nil)
			case "physical-copy":
				raw = recordFixture(true, words(10, 20), nil)
			case "duplicate-id":
				copy(raw[len(raw)-4:], raw[len(raw)-8:len(raw)-4])
			case "relation-ordinal":
				raw = recordFixture(true, nil, words(1, 0, 0, 5, 2))
			case "duplicate-relation":
				raw = recordFixture(true, nil, words(2, 0, 0, 5, 0, 6, 0))
			case "relation-size":
				raw = recordFixture(true, nil, words(2, 0, 0, 5, 0))
			case "short-id":
				binary.LittleEndian.PutUint32(raw[96:100], 4)
			case "sparse":
				binary.LittleEndian.PutUint16(raw[40:42], 1)
			case "budget":
				raw = recordFixture(true, words(100, 10), nil)
				budget.Rows = 2
			}
			if got, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), budget); err == nil || got != nil {
				t.Fatalf("accepted %s", name)
			}
		})
	}
}

type closingRecordSource struct {
	*bytes.Reader
	closed bool
}

func (s *closingRecordSource) ReadAt(dst []byte, offset int64) (int, error) {
	if s.closed {
		return 0, io.ErrClosedPipe
	}
	return s.Reader.ReadAt(dst, offset)
}

func TestRecordContextAndClosedSource(t *testing.T) {
	raw := recordFixture(false, nil, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), limits); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.Lookup(ctx, 1); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	source := &closingRecordSource{Reader: bytes.NewReader(raw)}
	r, err = table.OpenRecords(context.Background(), source, int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	source.closed = true
	if got, err := r.Lookup(context.Background(), 1); !errors.Is(err, io.ErrClosedPipe) || got.Data != nil {
		t.Fatalf("%+v %v", got, err)
	}
}

func FuzzRecordIndex(f *testing.F) {
	f.Add(recordFixture(false, nil, nil))
	f.Add(recordFixture(true, words(100, 10), words(1, 0, 0, 123, 0)))
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 1<<20 {
			t.Skip()
		}
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err == nil {
			_, _ = r.Lookup(context.Background(), 1)
			_, _ = r.Values(context.Background(), 10, 0, 1)
		}
	})
}
