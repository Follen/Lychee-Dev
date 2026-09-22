package relational

import (
	"context"
	"errors"
	"math"
	"reflect"
	"testing"
)

func TestStreamingAggregates(t *testing.T) {
	for _, test := range []struct {
		expression string
		values     []any
		want       any
	}{
		{"COUNT(*)", []any{nil, nil, nil}, int64(3)},
		{"COUNT(ID)", []any{nil, 1, nil}, int64(1)},
		{"COUNT(ID)", nil, int64(0)},
		{"SUM(ID)", nil, nil},
		{"AVG(ID)", []any{nil}, nil},
		{"MIN(ID)", []any{nil}, nil},
		{"MAX(ID)", []any{nil}, nil},
		{"SUM(ID)", []any{uint64(math.MaxUint64), int64(-1)}, uint64(math.MaxUint64 - 1)},
		{"SUM(ID)", []any{uint64(math.MaxUint64), 1, -1}, uint64(math.MaxUint64)},
		{"AVG(ID)", []any{1, 2, nil}, 1.5},
		{"MIN(ID)", []any{uint64(math.MaxUint64), int64(-1)}, int64(-1)},
		{"MAX(ID)", []any{uint64(math.MaxUint64), int64(-1)}, uint64(math.MaxUint64)},
		{"COUNT(DISTINCT ID)", []any{1, uint64(1), 1.0, nil, "1", false}, int64(3)},
		{"SUM(DISTINCT ID)", []any{1, 1.0, 2}, int64(3)},
		{"MAX(ID)", []any{"cat", "dog"}, "dog"},
	} {
		p, err := Compile(context.Background(), "SELECT "+test.expression+" FROM t")
		if err != nil {
			t.Fatal(err)
		}
		a, err := newAggregate(p.root.outputs[0].value)
		if err != nil {
			t.Fatal(err)
		}
		e := evaluation{ctx: context.Background(), remaining: 10000, retainedBytes: 10000}
		for _, v := range test.values {
			if err := a.add(&e, v); err != nil {
				t.Fatal(test.expression, err)
			}
		}
		got, err := a.result()
		if err != nil || !reflect.DeepEqual(got, test.want) {
			t.Fatalf("%s: %#v, %v; want %#v", test.expression, got, err, test.want)
		}
	}
}

func TestAggregateLimits(t *testing.T) {
	for _, expression := range []string{"COUNT(DISTINCT *)", "SUM(*)", "COUNT(t.*)", "SUM()"} {
		p, err := Compile(context.Background(), "SELECT "+expression+" FROM t")
		if err != nil {
			t.Fatal(err)
		}
		if _, err := newAggregate(p.root.outputs[0].value); !errors.Is(err, ErrType) {
			t.Fatal(expression, err)
		}
	}
	a := &aggregate{name: "SUM", integer: true}
	e := evaluation{ctx: context.Background(), remaining: 100}
	for _, v := range []any{uint64(math.MaxUint64), 1} {
		if err := a.add(&e, v); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := a.result(); !errors.Is(err, ErrNumericRange) {
		t.Fatal(err)
	}
	a = &aggregate{name: "COUNT", seen: map[string]struct{}{}}
	if err := a.add(&e, 1); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	e.ctx = ctx
	if err := a.add(&e, 1); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
