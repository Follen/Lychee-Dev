package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestIndexedJoinNumericDuplicatesNullsAndErrors(t *testing.T) {
	ctx := context.Background()
	resolve := func(_ context.Context, use TableUse) (Source, error) {
		rows := [][]any{{uint64(1)}, {nil}, {int64(2)}}
		if use.Name == "b" {
			rows = [][]any{{float64(1)}, {int64(1)}, {nil}}
		}
		return Source{Columns: []string{"ID"}, Scan: func(_ context.Context, y func([]any) error) error {
			for _, r := range rows {
				if err := y(r); err != nil {
					return err
				}
			}
			return nil
		}}, nil
	}
	p, _ := Compile(ctx, "SELECT a.ID,b.ID FROM a LEFT JOIN b ON b.ID=a.ID")
	result, err := p.Execute(ctx, resolve, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
	want := [][]any{{uint64(1), float64(1)}, {uint64(1), int64(1)}, {nil, nil}, {int64(2), nil}}
	if err != nil || !reflect.DeepEqual(result.Rows, want) {
		t.Fatalf("%#v %v", result, err)
	}
	bad := func(ctx context.Context, use TableUse) (Source, error) {
		s, _ := resolve(ctx, use)
		if use.Name == "b" {
			s.Scan = func(_ context.Context, y func([]any) error) error { return y([]any{"text"}) }
		}
		return s, nil
	}
	if result, err := p.Execute(ctx, bad, nil, Limits{Work: 10000, MemoryBytes: 1 << 20}); !errors.Is(err, ErrType) || result.Rows != nil {
		t.Fatalf("%+v %v", result, err)
	}
}

func TestEqualityJoinFitsLinearWorkBudget(t *testing.T) {
	ctx := context.Background()
	p, _ := Compile(ctx, "SELECT COUNT(*) FROM a JOIN b ON a.ID=b.ID")
	result, err := p.Execute(ctx, func(context.Context, TableUse) (Source, error) { return generatedSource(1000), nil }, nil, Limits{Work: 40000, MemoryBytes: 2 << 20})
	if err != nil || result.Rows[0][0] != int64(1000) {
		t.Fatalf("%+v %v", result, err)
	}
}

func TestEqualityIndexFallsBackWithoutConsumingRetainedBudget(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT a.ID FROM a JOIN b ON a.ID=b.ID")
	if err != nil {
		t.Fatal(err)
	}
	e := &evaluation{ctx: context.Background(), remaining: 1000, retainedBytes: 400}
	fields := []field{{qualifier: "a", name: "ID"}, {qualifier: "b", name: "ID"}}
	index, err := buildJoinIndex(e, p.root.links[0].condition, fields, 1, [][]any{{int64(1)}, {int64(2)}, {int64(3)}})
	if err != nil || index != nil || e.retainedBytes != 400 {
		t.Fatalf("index=%+v budget=%d err=%v", index, e.retainedBytes, err)
	}
}
