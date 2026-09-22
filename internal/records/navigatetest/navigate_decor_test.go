package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

// Domain group "decor". Decor 80 mirrors the legacy golden oracle
// (wowdata fixtures/golden/go/decor/get-80-retail.json and
// get-model-6033663-retail.json).

func decorFixtures() []fixtureTable {
	return []fixtureTable{{
		name: "HouseDecor", fileDataID: 341,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Name_lang", kind: 's', loc: true},
			{name: "ModelFileDataID", kind: 'i', bits: 32},
			{name: "ThumbnailFileDataID", kind: 'i', bits: 32},
			{name: "ItemID", kind: 'i', bits: 32},
			{name: "GameObjectID", kind: 'i', bits: 32},
			{name: "Type", kind: 'i', bits: 32, signed: true},
			{name: "ModelType", kind: 'i', bits: 32, signed: true},
		},
		rows: [][]any{
			{uint64(5), "Plain Bench", uint64(0), uint64(0), uint64(0), uint64(0), int64(0), int64(0)},
			{uint64(80), "Ornate Stonework Fireplace", uint64(6033663), uint64(7415905), uint64(235994), uint64(527886), int64(1), int64(1)},
			{uint64(81), "Twin Ornate Fireplace", uint64(6033663), uint64(1), uint64(2), uint64(3), int64(4), int64(5)},
		},
	}}
}

// Cross-check against the legacy decor oracle
// (wowdata fixtures/golden/go/decor/get-80-retail.json).
func TestDecorGetMatchesLegacyOracle(t *testing.T) {
	fx := newFixture(t, decorFixtures())
	byID, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{ID: uint32Pointer(80)})
	if err != nil {
		t.Fatal(err)
	}
	item := byID.Result.Item
	if item.ID != 80 || item.Name != "Ornate Stonework Fireplace" || item.ModelFileDataID != 6033663 ||
		item.ThumbnailFileDataID != 7415905 || item.ItemID != 235994 || item.GameObjectID != 527886 ||
		item.Type != 1 || item.ModelType != 1 {
		t.Fatalf("decor = %+v", item)
	}
	byItem, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{ItemID: uint32Pointer(235994)})
	if err != nil {
		t.Fatal(err)
	}
	if byItem.Result.Item.ID != 80 {
		t.Fatalf("item lookup = %+v", byItem.Result)
	}
	// Model lookup matches the legacy get-model oracle and keeps ambiguous
	// candidates traceable (the legacy map silently kept one).
	byModel, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{ModelFileDataID: uint32Pointer(6033663)})
	if err != nil {
		t.Fatal(err)
	}
	if byModel.Result.Item.ID != 80 || !byModel.Result.Partial {
		t.Fatalf("model lookup = %+v", byModel.Result)
	}
	if _, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{}); !errors.Is(err, records.ErrRequestConflict) {
		t.Fatalf("no key must be rejected, got %v", err)
	}
	two := uint32Pointer(80)
	if _, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{ID: two, ModelFileDataID: uint32Pointer(6033663)}); !errors.Is(err, records.ErrRequestConflict) {
		t.Fatalf("two keys must be rejected, got %v", err)
	}
	if _, err := records.InspectDecorGet(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorGetRequest{ID: uint32Pointer(404404)}); !errors.Is(err, records.ErrRecordNotFound) {
		t.Fatalf("missing decor must be explicit, got %v", err)
	}
}

func TestDecorListIsBoundedAndComplete(t *testing.T) {
	fx := newFixture(t, decorFixtures())
	full, err := records.InspectDecorList(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorListRequest{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if full.Result.Count != 3 || !full.Result.Complete || full.Result.Truncated {
		t.Fatalf("list = %+v", full.Result)
	}
	// Rows without a model file are kept (the legacy loader dropped them).
	if full.Result.Items[0].ID != 5 || full.Result.Items[0].ModelFileDataID != 0 || full.Result.Items[1].ID != 80 {
		t.Fatalf("items = %+v", full.Result.Items)
	}
	cut, err := records.InspectDecorList(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorListRequest{Limit: 2})
	if err != nil {
		t.Fatal(err)
	}
	if cut.Result.Count != 2 || !cut.Result.Truncated || cut.Result.Complete || cut.Result.TruncationReason != "limit_reached" {
		t.Fatalf("cut list = %+v", cut.Result)
	}
	if _, err := records.InspectDecorList(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.DecorListRequest{Limit: 0}); !errors.Is(err, records.ErrLimitRequired) {
		t.Fatalf("unbounded list must be rejected, got %v", err)
	}
}

func uint32Pointer(value uint32) *uint32 { return &value }
