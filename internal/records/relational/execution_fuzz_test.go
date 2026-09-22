package relational

import (
	"context"
	"testing"
)

func FuzzExecuteBounded(f *testing.F) {
	for _, sql := range []string{
		"SELECT ID FROM a WHERE ID IN (SELECT ID FROM b)",
		"EXPLAIN ANALYZE SELECT COUNT(*) FROM a LEFT JOIN b ON a.ID=b.ID",
		"WITH RECURSIVE x AS (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID+1 FROM x WHERE ID<4) SELECT * FROM x",
		"SELECT a.ID, (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) FROM a GROUP BY a.ID",
	} {
		f.Add(sql)
	}
	f.Fuzz(func(t *testing.T, sql string) {
		if len(sql) > 4096 {
			t.Skip()
		}
		p, err := Compile(context.Background(), sql)
		if err != nil {
			return
		}
		result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
		if err != nil && (result.Rows != nil || result.Columns != nil || result.Plan != nil) {
			t.Fatal("partial error result", result, err)
		}
		if err == nil && result.Plan == nil {
			for _, row := range result.Rows {
				if len(row) != len(result.Columns) {
					t.Fatal("result shape", result)
				}
			}
		}
	})
}
