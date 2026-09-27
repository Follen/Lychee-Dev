package navigatetest

import (
	"context"
	"github.com/follenfang/lycheedev/internal/records"
	"testing"
)

func TestSQLIdentityLookupPreservesPartialCoverage(t *testing.T) {
	fx := newFixture(t, []fixtureTable{encryptedSections(t)})
	for _, id := range []string{"1", "2", "999"} {
		got, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: "EXPLAIN ANALYZE SELECT Value FROM Sample WHERE ID=" + id})
		if err != nil {
			t.Fatal(err)
		}
		if got.Result.Complete || !got.Result.Partial || got.Capture.Complete {
			t.Fatal("lost incomplete source")
		}
		scans := got.Result.Result.Plan.Measured.Scans
		if len(scans) != 1 || scans[0].Lookups != 1 || scans[0].Rows > 1 {
			t.Fatalf("not a bounded lookup: %+v", scans)
		}
	}
}
