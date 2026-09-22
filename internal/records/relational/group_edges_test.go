package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func groupingForTest(t *testing.T, sql string) *grouping {
	t.Helper()
	p, err := Compile(context.Background(), sql)
	if err != nil {
		t.Fatal(err)
	}
	g, err := prepareGroups(p.root)
	if err != nil {
		t.Fatal(err)
	}
	return g
}

func TestGroupingEdgesCancellationAndBudgets(t *testing.T) {
	g := groupingForTest(t, "SELECT x, COUNT(*) FROM t GROUP BY x")

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	e := evaluation{ctx: ctx, remaining: 100, retainedBytes: 1 << 20}
	e.column = func(_, _ string) (any, error) { return "x", nil }
	if err := g.add(&e); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled group add: %v", err)
	}

	for _, test := range []struct {
		name string
		e    evaluation
	}{
		{name: "work", e: evaluation{ctx: context.Background(), remaining: 0, retainedBytes: 1 << 20}},
		{name: "memory", e: evaluation{ctx: context.Background(), remaining: 100, retainedBytes: 0}},
	} {
		t.Run(test.name, func(t *testing.T) {
			g := groupingForTest(t, "SELECT x, COUNT(*) FROM t GROUP BY x")
			test.e.column = func(_, _ string) (any, error) { return "x", nil }
			if err := g.add(&test.e); !errors.Is(err, ErrBudget) {
				t.Fatalf("budget error: %v", err)
			}
			if len(g.order) != 0 || len(g.groups) != 0 {
				t.Fatalf("budget failure retained a group: order=%d groups=%d", len(g.order), len(g.groups))
			}
		})
	}
}

func TestGroupingEdgesCompositeKeysDoNotCollide(t *testing.T) {
	g := groupingForTest(t, "SELECT a, b, COUNT(*) FROM t GROUP BY a, b")
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 1 << 20}
	rows := []map[string]any{{"a": "ab", "b": "c"}, {"a": "a", "b": "bc"}}
	for _, row := range rows {
		e.column = func(_, name string) (any, error) { return row[name], nil }
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	got, err := g.finish(&e)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 2 || !reflect.DeepEqual(got[0].values, []any{"ab", "c", int64(1)}) || !reflect.DeepEqual(got[1].values, []any{"a", "bc", int64(1)}) {
		t.Fatalf("composite groups collided or changed order: %#v", got)
	}
}

func TestGroupingEdgesNumericTypesShareGroup(t *testing.T) {
	g := groupingForTest(t, "SELECT x, COUNT(*) FROM t GROUP BY x")
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 1 << 20}
	for _, value := range []any{int8(1), uint64(1), float64(1)} {
		e.column = func(_, _ string) (any, error) { return value, nil }
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	got, err := g.finish(&e)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || !reflect.DeepEqual(got[0].values, []any{int64(1), int64(3)}) {
		t.Fatalf("numeric values did not share a group: %#v", got)
	}
}

func TestGroupingEdgesHavingNullFiltersGroup(t *testing.T) {
	g := groupingForTest(t, "SELECT x, COUNT(*) FROM t GROUP BY x HAVING NULL")
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 1 << 20}
	e.column = func(_, _ string) (any, error) { return "x", nil }
	if err := g.add(&e); err != nil {
		t.Fatal(err)
	}
	got, err := g.finish(&e)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 0 {
		t.Fatalf("HAVING NULL unexpectedly emitted rows: %#v", got)
	}
}

func TestGroupingEdgesErrorReturnsNoPartialOutput(t *testing.T) {
	g := groupingForTest(t, "SELECT x, MIN(y) + 1 FROM t GROUP BY x")
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 1 << 20}
	marker := &term{kind: "text", value: "marker"}
	resolved := map[*term]any{marker: "outside"}
	e.resolved = resolved
	rows := []map[string]any{{"x": "first", "y": int64(1)}, {"x": "second", "y": "not a number"}}
	for _, row := range rows {
		e.column = func(_, name string) (any, error) { return row[name], nil }
		if err := g.add(&e); err != nil {
			t.Fatal(err)
		}
	}
	got, err := g.finish(&e)
	if err == nil {
		t.Fatal("invalid projection unexpectedly succeeded")
	}
	if got != nil {
		t.Fatalf("error returned partial output: %#v", got)
	}
	if !reflect.DeepEqual(e.resolved, resolved) || e.resolved[marker] != "outside" {
		t.Fatalf("resolved scope was not restored after error: %#v", e.resolved)
	}
}

func TestGroupingEdgesRestoresResolvedScope(t *testing.T) {
	g := groupingForTest(t, "SELECT x, COUNT(*) FROM t GROUP BY x")
	marker := &term{kind: "text", value: "marker"}
	resolved := map[*term]any{marker: "outside"}
	e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 1 << 20, resolved: resolved}
	e.column = func(_, _ string) (any, error) { return "x", nil }
	if err := g.add(&e); err != nil {
		t.Fatal(err)
	}
	if _, err := g.finish(&e); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(e.resolved, resolved) || e.resolved[marker] != "outside" {
		t.Fatalf("resolved scope was not restored: %#v", e.resolved)
	}
}
