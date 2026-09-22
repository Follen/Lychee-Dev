package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestCorrelatedQueries(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"SELECT a.ID FROM a WHERE EXISTS (SELECT b.ID FROM b WHERE b.ID=a.ID) ORDER BY a.ID", [][]any{{int64(1)}, {int64(3)}}},
		{"SELECT a.ID,(SELECT COUNT(*) FROM b WHERE b.ID=a.ID) AS n FROM a ORDER BY a.ID", [][]any{{int64(1), int64(2)}, {int64(2), int64(0)}, {int64(3), int64(1)}}},
		{"SELECT a.ID FROM a WHERE a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID) ORDER BY a.ID", [][]any{{int64(1)}, {int64(3)}}},
		{"SELECT a.ID FROM a WHERE EXISTS (SELECT b.ID FROM b WHERE EXISTS (SELECT c.ID FROM b c WHERE c.ID=a.ID AND c.ID=b.ID)) ORDER BY a.ID", [][]any{{int64(1)}, {int64(3)}}},
		{"SELECT a.ID,(SELECT ID FROM b a WHERE a.ID=3) FROM a ORDER BY a.ID", [][]any{{int64(1), int64(3)}, {int64(2), int64(3)}, {int64(3), int64(3)}}},
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

func TestQualifierShadowDoesNotFallThrough(t *testing.T) {
	e := evaluation{outer: &rowScope{fields: []field{{"x", "ID"}}}}
	_, err := bindColumn(&e, []field{{"x", "Name"}}, &term{kind: "column", qualifier: "x", value: "ID"})
	if !errors.Is(err, ErrBinding) {
		t.Fatal(err)
	}
	_, err = bindColumn(&e, []field{{"a", "ID"}, {"b", "ID"}}, &term{kind: "column", value: "ID"})
	if !errors.Is(err, ErrBinding) {
		t.Fatal(err)
	}
}
