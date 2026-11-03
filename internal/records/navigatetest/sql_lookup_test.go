package navigatetest

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
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
		scans := got.Result.Result.Scans
		if len(scans) != 1 || scans[0].Lookups != 1 || scans[0].Rows > 1 {
			t.Fatalf("not a bounded lookup: %+v", scans)
		}
	}
}

func TestSQLIdentitySetUsesPreparedReaderIndex(t *testing.T) {
	ids := make([]string, 35)
	for i := range ids {
		ids[i] = fmt.Sprint(i + 1)
	}
	for _, local := range []bool{false, true} {
		fx := newFixture(t, benchmarkTables(1000))
		files := fixtureQuery()
		if local {
			files.CDN = false
			files.Installation = fixtureInstallation(t, fx)
		}
		for _, filter := range []string{"ID=35", "Value >= 0 AND ID IN (" + strings.Join(ids, ",") + ")"} {
			got, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, files, records.DataQuery{SQL: "SELECT ID,Value FROM Sample WHERE " + filter + " ORDER BY ID"})
			if err != nil {
				t.Fatal(err)
			}
			want := 35
			if filter == "ID=35" {
				want = 1
			}
			result := got.Result.Result
			if !got.Result.Complete || len(result.Rows) != want || len(result.Scans) != 1 || result.Scans[0].Rows != int64(want) || result.Scans[0].Lookups != int64(want) {
				t.Fatalf("local=%t filter=%s result=%+v", local, filter, result)
			}
			for i, row := range result.Rows {
				id := i + 1
				if want == 1 {
					id = 35
				}
				if fmt.Sprint(row) != fmt.Sprintf("[%d %d]", id, (id-1)*3) {
					t.Fatal(row)
				}
			}
		}
	}
}
