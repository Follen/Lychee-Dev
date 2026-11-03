package navigatetest

import (
	"context"
	"github.com/follenfang/lycheedev/internal/records"
	"strings"
	"testing"
)

func TestDB2UncorrelatedSubqueryScansRelatedOnce(t *testing.T) {
	fx := newFixture(t, benchmarkTables(100))
	result, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: "EXPLAIN ANALYZE SELECT s.ID,(SELECT COUNT(*) FROM Related) FROM Sample s ORDER BY s.ID"})
	if err != nil {
		t.Fatal(err)
	}
	plan := result.Result.Result.Plan
	if !result.Result.Complete || plan == nil || !plan.Analyze || plan.Engine != "duckdb" {
		t.Fatalf("%+v", result.Result)
	}
	found := false
	for _, scan := range result.Result.Result.Scans {
		if strings.EqualFold(scan.Table.Name, "related") {
			found = true
			if scan.Calls != 1 || scan.Rows != 100 {
				t.Fatalf("repeated related scan: %+v", scan)
			}
		}
	}
	if !found {
		t.Fatal("missing related scan evidence")
	}
	result, err = records.QueryData(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: "SELECT s.ID,(SELECT COUNT(*) FROM Related) FROM Sample s ORDER BY s.ID"})
	if err != nil || len(result.Result.Result.Rows) != 100 {
		t.Fatalf("rows=%d err=%v", len(result.Result.Result.Rows), err)
	}
	for _, row := range result.Result.Result.Rows {
		if row[1] != int64(100) {
			t.Fatalf("wrong count %v", row)
		}
	}
}
