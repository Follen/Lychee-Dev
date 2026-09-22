package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestCompoundQueries(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"WITH picked AS (SELECT ID, Name FROM a WHERE ID<3) SELECT Name FROM picked ORDER BY ID", [][]any{{"one"}, {"two"}}},
		{"WITH picked AS (SELECT ID FROM a WHERE ID<3), counted AS (SELECT COUNT(*) AS n FROM picked) SELECT n FROM counted", [][]any{{int64(2)}}},
		{"SELECT p.Name FROM (SELECT ID,Name FROM a WHERE ID=2) p", [][]any{{"two"}}},
		{"SELECT ID FROM a WHERE ID<3 UNION ALL SELECT ID FROM b ORDER BY ID DESC LIMIT 2", [][]any{{int64(3)}, {int64(2)}}},
		{"WITH picked AS (SELECT ID FROM a WHERE ID=2) SELECT ID FROM picked UNION ALL SELECT ID FROM picked", [][]any{{int64(2)}, {int64(2)}}},
	} {
		p, err := Compile(context.Background(), test.sql)
		if err != nil {
			t.Fatal(test.sql, err)
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if err != nil || !reflect.DeepEqual(result.Rows, test.want) {
			t.Fatal(test.sql, result, err)
		}
	}
}

func TestCTEShadowAndForwardReference(t *testing.T) {
	for _, sql := range []string{
		"WITH a AS (SELECT ID FROM a) SELECT ID FROM a",
		"WITH x AS (SELECT ID FROM y), y AS (SELECT ID FROM a) SELECT ID FROM x",
		"SELECT ID FROM a UNION ALL SELECT ID,Name FROM b",
	} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if !errors.Is(err, ErrBinding) || result.Rows != nil {
			t.Fatal(sql, result, err)
		}
	}
	p, err := Compile(context.Background(), "WITH a AS (SELECT ID FROM static.a) SELECT COUNT(*) FROM a")
	if err != nil {
		t.Fatal(err)
	}
	result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
	if err != nil || !reflect.DeepEqual(result.Rows, [][]any{{int64(3)}}) {
		t.Fatal(result, err)
	}
}
