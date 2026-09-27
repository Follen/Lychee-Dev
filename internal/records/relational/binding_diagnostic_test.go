package relational

import (
	"context"
	"errors"
	"testing"
)

func TestMissingColumnSuggestsScopedCandidatesWithoutScan(t *testing.T) {
	p, _ := Compile(context.Background(), "SELECT a.Nmae FROM a")
	_, err := p.Execute(context.Background(), func(context.Context, TableUse) (Source, error) {
		return Source{Columns: []string{"ID", "Name"}, Scan: func(context.Context, func([]any) error) error { t.Fatal("invalid query scanned"); return nil }}, nil
	}, nil, Limits{Work: 10000, MemoryBytes: 1 << 20})
	var d *BindingDiagnostic
	if !errors.As(err, &d) || len(d.Candidates) != 1 || d.Candidates[0] != "a.Name" {
		t.Fatalf("%v", err)
	}
}
