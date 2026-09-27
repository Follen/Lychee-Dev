package navigatetest

import (
	"context"
	"encoding/hex"
	"github.com/follenfang/lycheedev/internal/records"
	"testing"
)

func TestDecodedReuseDoesNotCacheMissingCoverage(t *testing.T) {
	spec := encryptedSections(t)
	fx := newFixture(t, []fixtureTable{spec})
	ctx := context.Background()
	q := fixtureQuery()
	q.Keys = nil // Public identity can cache unencrypted root only.
	first, err := records.InspectDataPage(ctx, fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	second, err := records.InspectDataPage(ctx, fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if first.Result.Complete || second.Result.Complete || second.Result.File.DecodedCache.Hits != 1 {
		t.Fatalf("partial data reused as complete: %+v", second.Result.File)
	}
	if _, err := hex.DecodeString(second.Result.File.PartialContent.SHA256); err != nil {
		t.Fatal(err)
	}
}
func TestDecodedReuseForFullSource(t *testing.T) {
	spec := fixtureTable{name: "Sample", fileDataID: 201, tableHash: 0xf00d0000, layoutHash: 0xabcd0000, columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Value", kind: 'i', bits: 32}}, rows: [][]any{{int64(1), int64(42)}}}
	fx := newFixture(t, []fixtureTable{spec})
	q := fixtureQuery()
	q.Keys = nil
	ctx := context.Background()
	first, err := records.InspectDataPage(ctx, fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	second, err := records.InspectDataPage(ctx, fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if !second.Result.Complete || first.Result.File.Content != second.Result.File.Content || second.Result.File.DecodedCache.Hits != 2 {
		t.Fatalf("reuse: %+v", second.Result.File)
	}
}
