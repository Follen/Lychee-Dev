package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestBoundedTopRowsPreserveStablePages(t *testing.T) {
	ctx := context.Background()
	for _, order := range []string{"", "ORDER BY Value", "ORDER BY Value DESC", "ORDER BY Value DESC NULLS FIRST"} {
		for _, limit := range []string{"LIMIT 0", "LIMIT 5", "LIMIT 5 OFFSET 3"} {
			sql := "SELECT ID,Value FROM a " + order + " " + limit
			p, _ := Compile(ctx, sql)
			resolver := func(context.Context, TableUse) (Source, error) {
				return Source{Columns: []string{"ID", "Value"}, Scan: func(_ context.Context, y func([]any) error) error {
					for i := 0; i < 40; i++ {
						var v any = int64(i % 3)
						if i%7 == 0 {
							v = nil
						}
						if err := y([]any{int64(i), v}); err != nil {
							return err
						}
					}
					return nil
				}}, nil
			}
			got, err := p.Execute(ctx, resolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
			if err != nil {
				t.Fatal(sql, err)
			}
			// DISTINCT is an independent full-result path; ID makes every row unique.
			reference, _ := Compile(ctx, "SELECT DISTINCT ID,Value FROM a "+order+" "+limit)
			want, err := reference.Execute(ctx, resolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
			if err != nil || !reflect.DeepEqual(got.Rows, want.Rows) {
				t.Fatalf("%s: %#v vs %#v %v", sql, got.Rows, want.Rows, err)
			}
		}
	}
}

func TestTopRowsBudgetAndLateError(t *testing.T) {
	ctx := context.Background()
	p, _ := Compile(ctx, "SELECT ID FROM a ORDER BY Value LIMIT 20")
	resolver := func(context.Context, TableUse) (Source, error) { return generatedSource(20000), nil }
	got, err := p.Execute(ctx, resolver, nil, Limits{Work: 1000000, MemoryBytes: 32 << 10})
	if err != nil || len(got.Rows) != 20 || got.Rows[0][0] != int64(19999) {
		t.Fatalf("%+v %v", got, err)
	}
	late := errors.New("late corrupt source")
	broken := func(ctx context.Context, u TableUse) (Source, error) {
		s, _ := resolver(ctx, u)
		scan := s.Scan
		s.Scan = func(ctx context.Context, y func([]any) error) error {
			if err := scan(ctx, y); err != nil {
				return err
			}
			return late
		}
		return s, nil
	}
	if r, err := p.Execute(ctx, broken, nil, Limits{Work: 1000000, MemoryBytes: 32 << 10}); !errors.Is(err, late) || r.Rows != nil {
		t.Fatalf("%+v %v", r, err)
	}
}
