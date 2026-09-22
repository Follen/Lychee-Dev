package relational

import (
	"context"
	"testing"
)

func TestPreflightDoesNotScanOnHiddenErrors(t *testing.T) {
	for _, sql := range []string{
		"SELECT (SELECT missing FROM b) FROM a WHERE ID=0",
		"SELECT CASE WHEN FALSE THEN (SELECT BAD() FROM b) ELSE 1 END FROM a",
		"SELECT ID FROM a WHERE ID IN (SELECT ID,Name FROM b WHERE ID=0)",
		"SELECT (SELECT missing FROM b WHERE ID=0) FROM a",
		"WITH RECURSIVE walk AS (SELECT ID FROM a WHERE ID=0 UNION ALL SELECT missing FROM walk) SELECT ID FROM walk",
		"SELECT ID FROM a WHERE EXISTS (SELECT b.ID FROM b WHERE b.ID=unknown.ID)",
		"SELECT COUNT(*),(SELECT a.Name FROM b WHERE ID=0) FROM a WHERE ID=0",
		"SELECT a.ID,(SELECT a.Name FROM b WHERE ID=0) FROM a GROUP BY a.ID",
	} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		scans := 0
		resolve := func(ctx context.Context, use TableUse) (Source, error) {
			source, err := fixtureResolver(ctx, use)
			if err != nil {
				return Source{}, err
			}
			source.Scan = func(context.Context, func([]any) error) error { scans++; return nil }
			return source, nil
		}
		result, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if err == nil || scans != 0 || result.Rows != nil {
			t.Fatal(sql, result, err, scans)
		}
	}
}

func TestSourceResolvedOnceAcrossPreflightAndSubqueries(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT ID,(SELECT COUNT(*) FROM b WHERE b.ID=a.ID) FROM a")
	if err != nil {
		t.Fatal(err)
	}
	opened := map[string]int{}
	resolve := func(ctx context.Context, use TableUse) (Source, error) {
		opened[use.Name]++
		return fixtureResolver(ctx, use)
	}
	result, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
	if err != nil || len(result.Rows) != 3 || opened["a"] != 1 || opened["b"] != 1 {
		t.Fatal(result, err, opened)
	}
}
