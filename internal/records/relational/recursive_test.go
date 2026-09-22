package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestRecursiveDelta(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"WITH RECURSIVE walk AS (SELECT ID AS n FROM a WHERE ID=1 UNION ALL SELECT n+1 FROM walk WHERE n<4) SELECT n FROM walk ORDER BY n", [][]any{{int64(1)}, {int64(2)}, {int64(3)}, {int64(4)}}},
		{"WITH RECURSIVE walk AS (SELECT ID AS n FROM a WHERE ID=0 UNION ALL SELECT n+1 FROM walk WHERE n<4) SELECT COUNT(*) FROM walk", [][]any{{int64(0)}}},
		{"WITH RECURSIVE plain AS (SELECT ID FROM a WHERE ID=2) SELECT ID FROM plain", [][]any{{int64(2)}}},
	} {
		p, err := Compile(context.Background(), test.sql)
		if err != nil {
			t.Fatal(err)
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if err != nil || !reflect.DeepEqual(result.Rows, test.want) {
			t.Fatal(test.sql, result, err)
		}
	}
}

func TestRecursiveCycleBudget(t *testing.T) {
	p, err := Compile(context.Background(), "WITH RECURSIVE walk AS (SELECT ID AS n FROM a WHERE ID=1 UNION ALL SELECT n FROM walk) SELECT n FROM walk")
	if err != nil {
		t.Fatal(err)
	}
	for _, limit := range []Limits{{Work: 200, MemoryBytes: 1 << 20}, {Work: 100000, MemoryBytes: 2048}} {
		result, err := p.Execute(context.Background(), fixtureResolver, nil, limit)
		if !errors.Is(err, ErrBudget) || result.Rows != nil {
			t.Fatal(result, err)
		}
	}
}

func TestRecursiveRejectsInvalidMembers(t *testing.T) {
	for _, sql := range []string{
		"WITH RECURSIVE walk AS (SELECT ID FROM walk UNION ALL SELECT ID FROM walk) SELECT ID FROM walk",
		"WITH RECURSIVE walk AS (SELECT ID FROM a UNION ALL SELECT x.ID FROM walk x JOIN walk y ON x.ID=y.ID) SELECT ID FROM walk",
	} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
		if !errors.Is(err, ErrBinding) || result.Rows != nil {
			t.Fatal(sql, result, err)
		}
	}
}
