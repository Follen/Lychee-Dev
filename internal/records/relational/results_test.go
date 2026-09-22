package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestResultStages(t *testing.T) {
	for _, test := range []struct {
		clause string
		want   []any
	}{
		{"ORDER BY x", []any{nil, int64(1), int64(2), int64(2)}},
		{"ORDER BY x DESC", []any{int64(2), int64(2), int64(1), nil}},
		{"ORDER BY x DESC NULLS FIRST", []any{nil, int64(2), int64(2), int64(1)}},
		{"ORDER BY x NULLS LAST LIMIT 2 OFFSET 1", []any{int64(2), int64(2)}},
		{"ORDER BY x LIMIT 0", []any{}},
		{"ORDER BY x OFFSET 18446744073709551615", []any{}},
	} {
		p, err := Compile(context.Background(), "SELECT x FROM t "+test.clause)
		if err != nil {
			t.Fatal(err)
		}
		input := []groupOutput{{values: []any{int64(2)}, order: []any{int64(2)}}, {values: []any{nil}, order: []any{nil}}, {values: []any{int64(1)}, order: []any{int64(1)}}, {values: []any{int64(2)}, order: []any{int64(2)}}}
		e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000}
		rows, err := finishResults(&e, p.root, input)
		if err != nil {
			t.Fatal(err)
		}
		got := []any{}
		for _, row := range rows {
			got = append(got, row.values[0])
		}
		if !reflect.DeepEqual(got, test.want) {
			t.Fatal(test.clause, got, test.want)
		}
		if input[0].values[0] != int64(2) {
			t.Fatal("mutated input order")
		}
	}
}

func TestDistinctBeforeLimit(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT DISTINCT x FROM t LIMIT :n OFFSET 1")
	if err != nil {
		t.Fatal(err)
	}
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000, parameters: map[string]any{"n": 1}}
	input := []groupOutput{{values: []any{1}}, {values: []any{1.0}}, {values: []any{2}}, {values: []any{nil}}, {values: []any{nil}}}
	rows, err := finishResults(&e, p.root, input)
	if err != nil || len(rows) != 1 || rows[0].values[0] != 2 {
		t.Fatal(rows, err)
	}
	e.parameters["n"] = -1
	if _, err := finishResults(&e, p.root, input); !errors.Is(err, ErrType) {
		t.Fatal(err)
	}
}

func TestOrderFailureIsAtomic(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT x FROM t ORDER BY x")
	if err != nil {
		t.Fatal(err)
	}
	input := []groupOutput{{values: []any{1}, order: []any{1}}, {values: []any{"a"}, order: []any{"a"}}}
	e := evaluation{ctx: context.Background(), remaining: 1000, retainedBytes: 100000}
	rows, err := finishResults(&e, p.root, input)
	if !errors.Is(err, ErrType) || rows != nil {
		t.Fatal(rows, err)
	}
	if input[0].values[0] != 1 {
		t.Fatal("mutated input")
	}
}

func TestGroupThenOrderAndPage(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT x, SUM(y) FROM t GROUP BY x HAVING SUM(y)>2 ORDER BY SUM(y) DESC LIMIT 1 OFFSET 1")
	if err != nil {
		t.Fatal(err)
	}
	g, err := prepareGroups(p.root)
	if err != nil {
		t.Fatal(err)
	}
	e := evaluation{ctx: context.Background(), remaining: 100000, retainedBytes: 1 << 20}
	for _, row := range []struct {
		x string
		y int
	}{{"a", 2}, {"b", 4}, {"a", 5}, {"c", 1}} {
		e.column = func(_, name string) (any, error) {
			if name == "x" {
				return row.x, nil
			}
			return row.y, nil
		}
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	projected, err := g.finish(&e)
	if err != nil {
		t.Fatal(err)
	}
	rows, err := finishResults(&e, p.root, projected)
	if err != nil || len(rows) != 1 || !reflect.DeepEqual(rows[0].values, []any{"b", int64(4)}) {
		t.Fatal(rows, err)
	}
}

func TestStableOrderAndCancellation(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT x FROM t ORDER BY x")
	if err != nil {
		t.Fatal(err)
	}
	input := []groupOutput{{values: []any{"first"}, order: []any{1}}, {values: []any{"second"}, order: []any{1}}, {values: []any{"third"}, order: []any{0}}}
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000}
	rows, err := finishResults(&e, p.root, input)
	if err != nil || rows[0].values[0] != "third" || rows[1].values[0] != "first" || rows[2].values[0] != "second" {
		t.Fatal(rows, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	e.ctx = ctx
	if rows, err := finishResults(&e, p.root, input); !errors.Is(err, context.Canceled) || rows != nil {
		t.Fatal(rows, err)
	}
	e.ctx = context.Background()
	e.retainedBytes = 0
	if rows, err := finishResults(&e, p.root, input); !errors.Is(err, ErrBudget) || rows != nil {
		t.Fatal(rows, err)
	}
}
