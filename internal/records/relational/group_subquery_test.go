package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestGroupSubqueries(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"SELECT a.ID, COUNT(*), (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) FROM a GROUP BY a.ID ORDER BY a.ID", [][]any{{int64(1), int64(1), int64(2)}, {int64(2), int64(1), int64(0)}, {int64(3), int64(1), int64(1)}}},
		{"SELECT a.ID,COUNT(*) FROM a GROUP BY a.ID HAVING EXISTS (SELECT ID FROM b WHERE b.ID=a.ID) ORDER BY a.ID", [][]any{{int64(1), int64(1)}, {int64(3), int64(1)}}},
		{"SELECT COUNT(*),(SELECT COUNT(*) FROM b) FROM a WHERE ID=9", [][]any{{int64(0), int64(3)}}},
		{"SELECT a.ID,COUNT(*) FROM a GROUP BY a.ID ORDER BY (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) DESC", [][]any{{int64(1), int64(1)}, {int64(3), int64(1)}, {int64(2), int64(1)}}},
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

func TestGroupSubqueryRejectsRepresentativeRow(t *testing.T) {
	for _, sql := range []string{
		"SELECT COUNT(*),(SELECT a.Name FROM b WHERE ID=3) FROM a",
		"SELECT a.ID,(SELECT a.Name FROM b WHERE ID=3) FROM a GROUP BY a.ID",
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
}
