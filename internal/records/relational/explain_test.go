package relational

import (
	"context"
	"testing"
)

func TestExplainDoesNotScan(t *testing.T) {
	p, err := Compile(context.Background(), "EXPLAIN SELECT a.ID FROM a LEFT JOIN b ON a.ID=b.ID ORDER BY a.ID LIMIT 2")
	if err != nil {
		t.Fatal(err)
	}
	resolve := func(ctx context.Context, use TableUse) (Source, error) {
		source, err := fixtureResolver(ctx, use)
		if err != nil {
			return Source{}, err
		}
		source.Scan = func(context.Context, func([]any) error) error { t.Fatal("EXPLAIN scanned rows"); return nil }
		return source, nil
	}
	result, err := p.Execute(context.Background(), resolve, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
	if err != nil || result.Plan == nil || result.Plan.Measured != nil || result.Rows != nil {
		t.Fatal(result, err)
	}
	operations := map[string]bool{}
	for _, step := range result.Plan.Steps {
		operations[step.Operation] = true
	}
	for _, op := range []string{"input", "nested-loop-join", "order", "slice"} {
		if !operations[op] {
			t.Fatal(op, result.Plan)
		}
	}
}

func TestAnalyzeActualScans(t *testing.T) {
	p, err := Compile(context.Background(), "EXPLAIN ANALYZE SELECT a.ID FROM a LEFT JOIN b ON a.ID=b.ID ORDER BY a.ID LIMIT 2")
	if err != nil {
		t.Fatal(err)
	}
	result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
	if err != nil || result.Plan == nil || result.Plan.Measured == nil {
		t.Fatal(result, err)
	}
	m := result.Plan.Measured
	if m.OutputRows != 2 || m.WorkUnits <= 0 || m.ChargedBytes <= 0 || len(m.Scans) != 2 {
		t.Fatal(m)
	}
	for _, scan := range m.Scans {
		if scan.Calls != 1 || scan.Rows != 3 {
			t.Fatal(scan)
		}
	}
}
