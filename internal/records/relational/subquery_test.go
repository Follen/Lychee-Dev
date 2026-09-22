package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestExpressionSubqueries(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"SELECT (SELECT Name FROM b WHERE ID=3) FROM a WHERE ID=1", [][]any{{"z"}}},
		{"SELECT (SELECT Name FROM b WHERE ID=9) FROM a WHERE ID=1", [][]any{{nil}}},
		{"SELECT ID FROM a WHERE EXISTS (SELECT ID FROM b WHERE ID=3) ORDER BY ID", [][]any{{int64(1)}, {int64(2)}, {int64(3)}}},
		{"SELECT ID FROM a WHERE ID IN (SELECT ID FROM b) ORDER BY ID", [][]any{{int64(1)}, {int64(3)}}},
		{"SELECT ID FROM a WHERE ID NOT IN (SELECT ID FROM b) ORDER BY ID", [][]any{{int64(2)}}},
		{"SELECT 2 NOT IN (SELECT NULL FROM b) FROM a WHERE ID=1", [][]any{{nil}}},
		{"SELECT NULL IN (SELECT ID FROM b WHERE ID=9) FROM a WHERE ID=1", [][]any{{false}}},
		{"WITH selected AS (SELECT ID FROM b WHERE ID=3) SELECT ID FROM a WHERE ID IN (SELECT ID FROM selected)", [][]any{{int64(3)}}},
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

func TestSubqueryCardinality(t *testing.T) {
	for _, sql := range []string{
		"SELECT (SELECT ID FROM b) FROM a",
		"SELECT (SELECT ID,Name FROM b WHERE ID=9) FROM a",
		"SELECT ID FROM a WHERE ID IN (SELECT ID,Name FROM b)",
	} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if !errors.Is(err, ErrCardinality) || result.Rows != nil {
			t.Fatal(sql, result, err)
		}
	}
}
