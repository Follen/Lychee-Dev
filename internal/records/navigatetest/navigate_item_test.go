package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

// Domain group "item". Item 25 mirrors the legacy golden oracles
// (wowdata fixtures/golden/go/item/get-25-classic-era.json,
// models-25-classic-era.json, geosets-25-classic-era.json,
// textures-7517-classic-era.json) so the summaries, model/texture file IDs and
// slot naming can be checked against them and against these hand-built rows.

func itemSparseFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "ItemSparse", fileDataID: 311,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Display_lang", kind: 's', loc: true},
			{name: "InventoryType", kind: 'i', bits: 32, signed: true},
			{name: "OverallQualityID", kind: 'i', bits: 32, signed: true},
		},
		rows: rows,
	}
}

func itemFixtures() []fixtureTable {
	return []fixtureTable{
		{
			name: "Item", fileDataID: 312,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ClassID", kind: 'i', bits: 32, signed: true},
				{name: "SubclassID", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(25), int64(2), int64(7)}, {uint64(26), int64(4), int64(0)}},
		},
		itemSparseFixture([][]any{
			{uint64(25), "Worn Shortsword", int64(21), int64(1)},
			{uint64(26), "Cloth Belt", int64(6), int64(1)},
			{uint64(27), "Twin Charm", int64(2), int64(2)},
		}),
		{
			name: "ItemModifiedAppearance", fileDataID: 313,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemID", kind: 'i', bits: 32},
				{name: "ItemAppearanceID", kind: 'i', bits: 32},
			},
			rows: [][]any{
				{uint64(5), uint64(25), uint64(3)},
				{uint64(6), uint64(26), uint64(4)},
				{uint64(7), uint64(27), uint64(3)},
				{uint64(8), uint64(27), uint64(4)},
			},
		},
		{
			name: "ItemAppearance", fileDataID: 314,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemDisplayInfoID", kind: 'i', bits: 32},
			},
			rows: [][]any{{uint64(3), uint64(1542)}, {uint64(4), uint64(1543)}},
		},
		{
			name: "ItemDisplayInfo", fileDataID: 315,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ModelResourcesID", kind: 'i', bits: 32, elements: 1},
				{name: "ModelMaterialResourcesID", kind: 'i', bits: 32, elements: 1},
				{name: "GeosetGroup", kind: 'i', bits: 32, signed: true, elements: 6},
				{name: "HelmetGeosetVis", kind: 'i', bits: 32, signed: true, elements: 2},
			},
			rows: [][]any{
				{uint64(1542), []int64{7}, []int64{8}, []int64{0, 0, 0, 0, 0, 0}, []int64{0, 0}},
				{uint64(1543), []int64{0}, []int64{0}, []int64{0, 0, 0, 0, 0, 0}, []int64{3, 0}},
			},
		},
		{
			name: "ModelFileData", fileDataID: 316,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "FileDataID", kind: 'i', bits: 32},
				{name: "ModelResourcesID", kind: 'i', bits: 32},
			},
			rows: [][]any{{uint64(1), uint64(148132), uint64(7)}, {uint64(2), uint64(999999), uint64(7)}},
		},
		{
			name: "TextureFileData", fileDataID: 317,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "FileDataID", kind: 'i', bits: 32},
				{name: "UsageType", kind: 'i', bits: 32},
				{name: "MaterialResourcesID", kind: 'i', bits: 32},
			},
			rows: [][]any{
				{uint64(1), uint64(148134), uint64(0), uint64(8)},
				{uint64(2), uint64(162971), uint64(0), uint64(9)},
				{uint64(3), uint64(555), uint64(1), uint64(8)},
			},
		},
		{
			name: "ComponentModelFileData", fileDataID: 318,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "RaceID", kind: 'i', bits: 32, signed: true},
				{name: "GenderIndex", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(148132), int64(1), int64(0)}, {uint64(999999), int64(1), int64(1)}},
		},
		{
			name: "ItemDisplayInfoMaterialRes", fileDataID: 319,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemDisplayInfoID", kind: 'i', bits: 32},
				{name: "ComponentSection", kind: 'i', bits: 32, signed: true},
				{name: "MaterialResourcesID", kind: 'i', bits: 32},
			},
			rows: [][]any{
				{uint64(1), uint64(1542), int64(0), uint64(8)},
				{uint64(2), uint64(1542), int64(1), uint64(9)},
			},
		},
		{
			name: "HelmetGeosetData", fileDataID: 320,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "HelmetGeosetVisDataID", kind: 'i', bits: 32},
				{name: "HideGeosetGroup", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(1), uint64(3), int64(2)}, {uint64(2), uint64(3), int64(5)}},
		},
	}
}

// Cross-check against the legacy item oracle
// (wowdata fixtures/golden/go/item/get-25-classic-era.json).
func TestItemGetMatchesLegacySummary(t *testing.T) {
	fx := newFixture(t, itemFixtures())
	reading, err := records.InspectItem(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemGetRequest{ItemID: 25})
	if err != nil {
		t.Fatal(err)
	}
	item := reading.Result.Item
	if item.ID != 25 || item.Name != "Worn Shortsword" || item.InventoryType != 21 ||
		item.ClassID != 2 || item.SubclassID != 7 || item.Quality != 1 || item.SlotName != "Main Hand" {
		t.Fatalf("item = %+v", item)
	}
	belt, err := records.InspectItem(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemGetRequest{ItemID: 26})
	if err != nil {
		t.Fatal(err)
	}
	if belt.Result.Item.SlotName != "Waist" {
		t.Fatalf("slot naming = %+v", belt.Result.Item)
	}
	if _, err := records.InspectItem(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemGetRequest{ItemID: 999}); !errors.Is(err, records.ErrRecordNotFound) {
		t.Fatalf("missing item must be an explicit failure, got %v", err)
	}
}

// Cross-check against the legacy models oracle
// (wowdata fixtures/golden/go/item/models-25-classic-era.json): display 1542,
// models [148132], textures [148134].
func TestItemModelsMatchLegacyOracleAndRaceGenderVariants(t *testing.T) {
	fx := newFixture(t, itemFixtures())
	reading, err := records.InspectItemModels(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemModelsRequest{ItemID: 25, RaceID: 1, Gender: 0})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.DisplayID != 1542 || len(result.Models) != 1 || result.Models[0] != 148132 {
		t.Fatalf("models = %+v", result)
	}
	if len(result.Textures) != 1 || result.Textures[0] != 148134 {
		t.Fatalf("textures = %+v", result)
	}
	if len(result.GeosetGroup) != 6 {
		t.Fatalf("geoset group = %v", result.GeosetGroup)
	}
	// The legacy loader ignored race/gender; 2.0 selects the matching variant
	// and falls back visibly.
	variant, err := records.InspectItemModels(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemModelsRequest{ItemID: 25, RaceID: 1, Gender: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(variant.Result.Models) != 1 || variant.Result.Models[0] != 999999 {
		t.Fatalf("race/gender variant = %+v", variant.Result)
	}
	missing, err := records.InspectItemModels(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemModelsRequest{ItemID: 25, RaceID: 7, Gender: 7})
	if err != nil {
		t.Fatal(err)
	}
	if missing.Result.Models[0] != 148132 || !missing.Result.Partial {
		t.Fatalf("fallback must be marked partial: %+v", missing.Result.DataContext)
	}
	// Ambiguous appearance rows keep every candidate in the relations and pick
	// the lowest row deterministically (legacy silently kept the last one).
	twin, err := records.InspectItemModels(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemModelsRequest{ItemID: 27})
	if err != nil {
		t.Fatal(err)
	}
	if twin.Result.DisplayID != 1542 || !twin.Result.Partial {
		t.Fatalf("ambiguous appearance = %+v", twin.Result)
	}
	candidates := 0
	for _, link := range twin.Result.Relations {
		if link.FromTable == "ItemModifiedAppearance" && link.ToKind == "item_appearance" {
			candidates++
		}
	}
	if candidates != 2 {
		t.Fatalf("all candidates must stay traceable: %+v", twin.Result.Relations)
	}
}

// Cross-check against the legacy geoset oracle
// (wowdata fixtures/golden/go/item/geosets-25-classic-era.json).
func TestItemGeosetsMatchLegacyOracle(t *testing.T) {
	fx := newFixture(t, itemFixtures())
	reading, err := records.InspectItemGeosets(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemGeosetRequest{ItemID: 25})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if len(result.GeosetGroup) != 6 || len(result.HelmetGeosetVis) != 2 || result.HelmetGeosetVis[0] != 0 {
		t.Fatalf("geosets = %+v", result)
	}
	if len(result.HelmetHide) != 0 {
		t.Fatalf("helmet hide = %v", result.HelmetHide)
	}
	// Helmet hide groups resolve per HelmetGeosetData row in row order.
	belt, err := records.InspectItemGeosets(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemGeosetRequest{ItemID: 26})
	if err != nil {
		t.Fatal(err)
	}
	if len(belt.Result.HelmetGeosetVis) != 2 || belt.Result.HelmetGeosetVis[0] != 3 {
		t.Fatalf("helmet vis = %+v", belt.Result)
	}
	if len(belt.Result.HelmetHide) != 2 || belt.Result.HelmetHide[0] != 2 || belt.Result.HelmetHide[1] != 5 {
		t.Fatalf("helmet hide = %v", belt.Result.HelmetHide)
	}
}

// Cross-check against the legacy textures oracle
// (wowdata fixtures/golden/go/item/textures-7517-classic-era.json): sections
// keep table order and only UsageType 0 textures qualify.
func TestItemTexturesMatchLegacyOracle(t *testing.T) {
	fx := newFixture(t, itemFixtures())
	reading, err := records.InspectItemTextures(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.ItemTexturesRequest{ItemID: 25})
	if err != nil {
		t.Fatal(err)
	}
	textures := reading.Result.Textures
	if len(textures) != 2 {
		t.Fatalf("textures = %+v", textures)
	}
	if textures[0] != (records.ItemTextureSection{Section: 0, FileDataID: 148134}) ||
		textures[1] != (records.ItemTextureSection{Section: 1, FileDataID: 162971}) {
		t.Fatalf("textures = %+v", textures)
	}
}
