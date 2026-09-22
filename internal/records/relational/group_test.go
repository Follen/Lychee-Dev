package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestGroupingPipeline(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT kind, COUNT(*), SUM(amount), AVG(amount) FROM t GROUP BY kind HAVING COUNT(*) > 1 ORDER BY SUM(amount) DESC")
	if err != nil {
		t.Fatal(err)
	}
	g, err := prepareGroups(p.root)
	if err != nil {
		t.Fatal(err)
	}
	e := evaluation{ctx: context.Background(), remaining: 100000, retainedBytes: 1 << 20}
	for _, row := range []map[string]any{{"kind": "a", "amount": 2}, {"kind": "b", "amount": 9}, {"kind": "a", "amount": 4}, {"kind": nil, "amount": nil}, {"kind": nil, "amount": 3}} {
		e.column = func(_, name string) (any, error) {
			v, ok := row[name]
			if !ok {
				return nil, ErrBinding
			}
			return v, nil
		}
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	got, err := g.finish(&e)
	if err != nil {
		t.Fatal(err)
	}
	want := []groupOutput{{values: []any{"a", int64(2), int64(6), float64(3)}, order: []any{int64(6)}}, {values: []any{nil, int64(2), int64(3), float64(3)}, order: []any{int64(3)}}}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("%#v != %#v", got, want)
	}
	if e.resolved != nil {
		t.Fatal("evaluation scope leaked")
	}
}

func TestGroupEmptyAndInvalid(t *testing.T) {
	for _, sql := range []string{"SELECT COUNT(*), SUM(x) FROM t", "SELECT COUNT(*), SUM(x) FROM t GROUP BY x"} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		g, err := prepareGroups(p.root)
		if err != nil {
			t.Fatal(err)
		}
		e := evaluation{ctx: context.Background(), remaining: 1000, retainedBytes: 100000}
		rows, err := g.finish(&e)
		if err != nil {
			t.Fatal(err)
		}
		if len(p.root.groups) == 0 {
			if len(rows) != 1 || !reflect.DeepEqual(rows[0].values, []any{int64(0), nil}) {
				t.Fatal(rows)
			}
		} else if len(rows) != 0 {
			t.Fatal(rows)
		}
	}
	for _, sql := range []string{"SELECT x, SUM(y) FROM t", "SELECT SUM(COUNT(*)) FROM t", "SELECT x FROM t GROUP BY SUM(x)", "SELECT * FROM t GROUP BY x"} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := prepareGroups(p.root); err == nil {
			t.Fatal("accepted", sql)
		}
	}
}

func TestGroupExpressionAndBudget(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT LOWER(x), COUNT(DISTINCT y) FROM t GROUP BY LOWER(x)")
	if err != nil {
		t.Fatal(err)
	}
	g, err := prepareGroups(p.root)
	if err != nil {
		t.Fatal(err)
	}
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 100000}
	for _, text := range []string{"CAT", "cat"} {
		e.column = func(_, name string) (any, error) {
			if name == "x" {
				return text, nil
			}
			return 1, nil
		}
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	rows, err := g.finish(&e)
	if err != nil || len(rows) != 1 || !reflect.DeepEqual(rows[0].values, []any{"cat", int64(1)}) {
		t.Fatal(rows, err)
	}
	g, err = prepareGroups(p.root)
	if err != nil {
		t.Fatal(err)
	}
	e.retainedBytes = 0
	if err := g.add(&e); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
}
