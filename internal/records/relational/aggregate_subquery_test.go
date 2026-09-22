package relational

import (
	"context"
	"reflect"
	"testing"
)

func TestAggregateInputSubqueries(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"SELECT SUM((SELECT COUNT(*) FROM b WHERE b.ID=a.ID)) FROM a", [][]any{{int64(3)}}},
		{"SELECT a.ID,SUM((SELECT COUNT(*) FROM b WHERE b.ID=a.ID)) FROM a GROUP BY a.ID ORDER BY a.ID", [][]any{{int64(1), int64(2)}, {int64(2), int64(0)}, {int64(3), int64(1)}}},
		{"SELECT COUNT(*) FROM a GROUP BY (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) ORDER BY COUNT(*)", [][]any{{int64(1)}, {int64(1)}, {int64(1)}}},
		{"SELECT (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) AS n, COUNT(*) FROM a GROUP BY (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) ORDER BY n", [][]any{{int64(0), int64(1)}, {int64(1), int64(1)}, {int64(2), int64(1)}}},
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
