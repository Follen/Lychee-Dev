package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

// Domain group "creature". Display 21 and model file 124578 mirror the legacy
// golden oracles (wowdata fixtures/golden/go/creature/display-21-classic-era.json
// and model-124578-classic-era.json).

func creatureFixtures() []fixtureTable {
	return []fixtureTable{
		{
			name: "CreatureDisplayInfo", fileDataID: 321,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ModelID", kind: 'i', bits: 32},
				{name: "TextureVariationFileDataID", kind: 'i', bits: 32, elements: 1},
			},
			rows: [][]any{
				{uint64(21), uint64(22), []int64{124577}},
				{uint64(22), uint64(22), []int64{124577}},
				{uint64(561), uint64(22), []int64{0}},
			},
		},
		{
			name: "CreatureModelData", fileDataID: 322,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "FileDataID", kind: 'i', bits: 32},
			},
			rows: [][]any{{uint64(22), uint64(124578)}},
		},
		{
			name: "CreatureDisplayInfoGeosetData", fileDataID: 323,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "CreatureDisplayInfoID", kind: 'i', bits: 32},
				{name: "GeosetIndex", kind: 'i', bits: 32, signed: true},
				{name: "GeosetValue", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(1), uint64(21), int64(1), int64(2)}},
		},
	}
}

// Cross-check against the legacy creature display oracle
// (wowdata fixtures/golden/go/creature/display-21-classic-era.json).
func TestCreatureDisplayResolvesBothKeys(t *testing.T) {
	fx := newFixture(t, creatureFixtures())
	byID, err := records.InspectCreatureDisplay(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureDisplayRequest{DisplayID: 21})
	if err != nil {
		t.Fatal(err)
	}
	result := byID.Result
	if result.DisplayID != 21 || result.ModelID != 22 || result.FileDataID != 124578 || result.ModelFileDataID != 124578 {
		t.Fatalf("display = %+v", result)
	}
	if len(result.Textures) != 1 || result.Textures[0] != 124577 {
		t.Fatalf("textures = %v", result.Textures)
	}
	if len(result.Variations) != 1 || result.Variations[0] != 202 {
		t.Fatalf("geoset variants = %v", result.Variations)
	}
	byFile, err := records.InspectCreatureDisplay(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureDisplayRequest{FileDataID: 124578})
	if err != nil {
		t.Fatal(err)
	}
	if byFile.Result.DisplayID != 21 {
		t.Fatalf("file lookup must return the first display: %+v", byFile.Result)
	}
	if _, err := records.InspectCreatureDisplay(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureDisplayRequest{}); !errors.Is(err, records.ErrRequestConflict) {
		t.Fatalf("exactly one key is required, got %v", err)
	}
	if _, err := records.InspectCreatureDisplay(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureDisplayRequest{DisplayID: 21, FileDataID: 124578}); !errors.Is(err, records.ErrRequestConflict) {
		t.Fatalf("two keys are ambiguous, got %v", err)
	}
	if _, err := records.InspectCreatureDisplay(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureDisplayRequest{DisplayID: 999}); !errors.Is(err, records.ErrRecordNotFound) {
		t.Fatalf("missing display must be explicit, got %v", err)
	}
}

// Cross-check against the legacy creature model oracle
// (wowdata fixtures/golden/go/creature/model-124578-classic-era.json):
// displays of one model file in ascending display order.
func TestCreatureModelListsDisplaysInOrder(t *testing.T) {
	fx := newFixture(t, creatureFixtures())
	reading, err := records.InspectCreatureModel(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureModelRequest{FileDataID: 124578})
	if err != nil {
		t.Fatal(err)
	}
	displays := reading.Result.Displays
	if len(displays) != 3 {
		t.Fatalf("displays = %+v", displays)
	}
	for index, want := range []uint32{21, 22, 561} {
		if displays[index].DisplayID != want || displays[index].ModelID != 22 || displays[index].FileDataID != 124578 {
			t.Fatalf("display %d = %+v", index, displays[index])
		}
	}
	if len(displays[0].Textures) != 1 || len(displays[2].Textures) != 0 {
		t.Fatalf("textures per display = %+v", displays)
	}
	if len(displays[0].Variations) != 1 || len(displays[1].Variations) != 0 {
		t.Fatalf("variants per display = %+v", displays)
	}
	empty, err := records.InspectCreatureModel(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.CreatureModelRequest{FileDataID: 404404})
	if err != nil {
		t.Fatal(err)
	}
	if len(empty.Result.Displays) != 0 || !empty.Result.Complete {
		t.Fatalf("unknown model file = %+v", empty.Result)
	}
}
