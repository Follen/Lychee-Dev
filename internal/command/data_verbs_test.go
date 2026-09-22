package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"testing"
)

// dataFixtureTables builds one workspace holding every table the pinned-data
// CLI verbs read, mirroring the legacy domain fixtures at hand-checkable size.
func dataFixtureTables(t *testing.T) []fixtureTable {
	t.Helper()
	return []fixtureTable{
		{
			name: "Map", fileDataID: 101, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Name_lang", kind: 's', loc: true},
				{name: "ParentMapID", kind: 'i', bits: 32, foreign: "Map::ID"},
			},
			rows: [][]any{{1, fixtureStr("Stormwind"), 0}, {2, fixtureStr("Duskwood"), 1}, {3, fixtureStr("Elwynn"), 1}},
		},
		{
			name: "SpellName", fileDataID: 102, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Name_lang", kind: 's', loc: true},
			},
			rows: [][]any{{100, fixtureStr("Charge")}, {200, fixtureStr("Aura Shield")}, {300, fixtureStr("Summon Dummy")}, {700, fixtureStr("Fireball")}},
		},
		{
			name: "Spell", fileDataID: 103, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Description_lang", kind: 's', loc: true},
				{name: "AuraDescription_lang", kind: 's', loc: true},
			},
			rows: [][]any{{100, fixtureStr("$@spellname7922"), fixtureStr("")}, {200, fixtureStr(""), fixtureStr("")}, {300, fixtureStr(""), fixtureStr("")}, {700, fixtureStr(""), fixtureStr("")}},
		},
		{
			name: "SpellEffect", fileDataID: 104, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "SpellID", kind: 'i', bits: 32},
				{name: "EffectIndex", kind: 'i', bits: 8},
				{name: "Effect", kind: 'i', bits: 32, signed: true},
				{name: "EffectAura", kind: 'i', bits: 32, signed: true},
				{name: "EffectTriggerSpell", kind: 'i', bits: 32},
				{name: "EffectAuraPeriod", kind: 'i', bits: 32},
				{name: "EffectBasePointsF", kind: 'f'},
				{name: "EffectMechanic", kind: 'i', bits: 32, signed: true},
				{name: "ImplicitTarget", kind: 'i', bits: 32, signed: true, elements: 2},
				{name: "EffectRadiusIndex", kind: 'i', bits: 32, signed: true, elements: 2},
				{name: "EffectMiscValue", kind: 'i', bits: 32, signed: true, elements: 2},
			},
			rows: [][]any{
				{1, 100, 0, 64, 0, 0, 0, float32(0), 0, fixtureArr(0, 0), fixtureArr(0, 0), fixtureArr(7922, 0)},
				{2, 100, 1, 2, 0, 7922, 0, float32(0), 0, fixtureArr(0, 0), fixtureArr(0, 0), fixtureArr(0, 0)},
				{3, 200, 0, 6, 0, 0, 0, float32(0), 0, fixtureArr(0, 0), fixtureArr(0, 0), fixtureArr(0, 0)},
				{4, 300, 0, 28, 0, 0, 0, float32(0), 0, fixtureArr(0, 0), fixtureArr(0, 0), fixtureArr(5000, 0)},
				{5, 7922, 0, 6, 0, 0, 0, float32(0), 0, fixtureArr(0, 0), fixtureArr(0, 0), fixtureArr(0, 0)},
			},
		},
		{
			name: "SpellMisc", fileDataID: 105, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "SpellID", kind: 'i', bits: 32},
				{name: "Attributes", kind: 'i', bits: 32, elements: 2},
				{name: "SchoolMask", kind: 'i', bits: 32, signed: true},
				{name: "Speed", kind: 'f'},
				{name: "SpellIconFileDataID", kind: 'i', bits: 32},
				{name: "CastingTimeIndex", kind: 'i', bits: 32},
				{name: "DurationIndex", kind: 'i', bits: 32},
				{name: "RangeIndex", kind: 'i', bits: 32},
			},
			rows: [][]any{{1, 100, fixtureArr(0, 0), 1, float32(0), 0, 1, 1, 1}},
		},
		{
			name: "SpellCastTimes", fileDataID: 106, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Base", kind: 'i', bits: 32, signed: true},
				{name: "Minimum", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{1, 1500, 1000}},
		},
		{
			name: "SpellDuration", fileDataID: 107, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Duration", kind: 'i', bits: 32, signed: true},
				{name: "MaxDuration", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{1, 5000, 5000}},
		},
		{
			name: "SpellRange", fileDataID: 108, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "DisplayName_lang", kind: 's', loc: true},
				{name: "RangeMin", kind: 'i', bits: 32, signed: true, elements: 2},
				{name: "RangeMax", kind: 'i', bits: 32, signed: true, elements: 2},
			},
			rows: [][]any{{1, fixtureStr("100 yards"), fixtureArr(0, 0), fixtureArr(100, 0)}},
		},
		{
			name: "ItemSparse", fileDataID: 109, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Display_lang", kind: 's', loc: true},
				{name: "InventoryType", kind: 'i', bits: 32, signed: true},
				{name: "OverallQualityID", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{9001, fixtureStr("Fixture Blade"), 13, 3}},
		},
		{
			name: "Item", fileDataID: 110, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ClassID", kind: 'i', bits: 32, signed: true},
				{name: "SubclassID", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{9001, 2, 5}},
		},
		{
			name: "ItemModifiedAppearance", fileDataID: 111, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemID", kind: 'i', bits: 32},
				{name: "ItemAppearanceID", kind: 'i', bits: 32},
			},
			rows: [][]any{{1, 9001, 7}},
		},
		{
			name: "ItemAppearance", fileDataID: 112, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemDisplayInfoID", kind: 'i', bits: 32},
			},
			rows: [][]any{{7, 21}},
		},
		{
			name: "ItemDisplayInfo", fileDataID: 113, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ModelResourcesID", kind: 'i', bits: 32, elements: 2},
				{name: "ModelMaterialResourcesID", kind: 'i', bits: 32, elements: 2},
				{name: "GeosetGroup", kind: 'i', bits: 32, signed: true, elements: 3},
				{name: "HelmetGeosetVis", kind: 'i', bits: 32, signed: true, elements: 3},
			},
			rows: [][]any{{21, fixtureArr(31, 0), fixtureArr(41, 0), fixtureArr(1, 2, 3), fixtureArr(0, 0, 0)}},
		},
		{
			name: "ModelFileData", fileDataID: 114, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ModelResourcesID", kind: 'i', bits: 32},
				{name: "FileDataID", kind: 'i', bits: 32},
			},
			rows: [][]any{{61, 31, 61001}},
		},
		{
			name: "TextureFileData", fileDataID: 115, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "MaterialResourcesID", kind: 'i', bits: 32},
				{name: "FileDataID", kind: 'i', bits: 32},
				{name: "UsageType", kind: 'i', bits: 32},
			},
			rows: [][]any{{71, 41, 71001, 0}},
		},
		{
			name: "ComponentModelFileData", fileDataID: 116, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "RaceID", kind: 'i', bits: 32, signed: true},
				{name: "GenderIndex", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{91, 4, 1}},
		},
		{
			name: "ItemDisplayInfoMaterialRes", fileDataID: 117, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ItemDisplayInfoID", kind: 'i', bits: 32},
				{name: "MaterialResourcesID", kind: 'i', bits: 32},
				{name: "ComponentSection", kind: 'i', bits: 32},
			},
			rows: [][]any{{81, 21, 41, 3}},
		},
		{
			name: "HelmetGeosetData", fileDataID: 118, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "HelmetGeosetVisDataID", kind: 'i', bits: 32},
				{name: "HideGeosetGroup", kind: 'i', bits: 32, signed: true, elements: 1},
			},
			rows: [][]any{{95, 0, fixtureArr(0)}},
		},
		{
			name: "CreatureDisplayInfo", fileDataID: 119, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "ModelID", kind: 'i', bits: 32},
				{name: "TextureVariationFileDataID", kind: 'i', bits: 32, elements: 2},
			},
			rows: [][]any{{400, 401, fixtureArr(41000, 0)}},
		},
		{
			name: "CreatureModelData", fileDataID: 120, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "FileDataID", kind: 'i', bits: 32},
			},
			rows: [][]any{{401, 42000}},
		},
		{
			name: "CreatureDisplayInfoGeosetData", fileDataID: 121, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "CreatureDisplayInfoID", kind: 'i', bits: 32},
				{name: "GeosetIndex", kind: 'i', bits: 8},
				{name: "GeosetValue", kind: 'i', bits: 8},
			},
			rows: [][]any{{1, 400, 0, 1}},
		},
		{
			name: "JournalEncounterSection", fileDataID: 122, columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "JournalEncounterID", kind: 'i', bits: 32},
				{name: "Title_lang", kind: 's', loc: true},
				{name: "BodyText_lang", kind: 's', loc: true},
				{name: "SpellID", kind: 'i', bits: 32},
				{name: "IconFlags", kind: 'i', bits: 32, signed: true},
				{name: "Type", kind: 'i', bits: 32, signed: true},
				{name: "DifficultyMask", kind: 'i', bits: 32, signed: true},
				{name: "IconCreatureDisplayInfoID", kind: 'i', bits: 32},
				{name: "OrderIndex", kind: 'i', bits: 32},
				{name: "ParentSectionID", kind: 'i', bits: 32},
				{name: "FirstChildSectionID", kind: 'i', bits: 32},
				{name: "NextSiblingSectionID", kind: 'i', bits: 32},
			},
			rows: [][]any{
				{1, 600, fixtureStr("Phase One"), fixtureStr("body"), 700, 0, 0, 0, 0, 0, 0, 2, 0},
				{2, 600, fixtureStr("Sub Section"), fixtureStr("body two"), 0, 0, 0, 0, 0, 1, 1, 0, 0},
			},
		},
		{
			name: "HouseDecor", fileDataID: 123, columns: []fixtureColumn{
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
				{50, fixtureStr("Fixture Lamp"), 500, 501, 900, 0, 1, 0},
				{51, fixtureStr("Fixture Rug"), 502, 503, 901, 0, 1, 0},
			},
		},
	}
}

func dataCommandFixture(t *testing.T) (root, pin string) {
	t.Helper()
	root, pin = newDataFixture(t, dataFixtureTables(t))
	return root, pin
}

// dataArgs prefixes the shared pinned-CDN offline source selection.
func dataArgs(pin string, extra ...string) []string {
	return append([]string{"--snapshot", pin, "--cdn", "--offline", "--home"}, extra...)
}

func TestDataTableVerbsCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	schema, code := invoke(t, append([]string{"data", "db2", "schema", "Map"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !schema.OK || len(schema.Captures) != 1 || schema.Context["snapshot"] != pin {
		t.Fatalf("schema: %d %+v", code, schema)
	}
	parsed := schema.Result.(map[string]any)
	if parsed["table"] != "Map" || parsed["rowCount"] != float64(3) || len(parsed["fields"].([]any)) != 3 {
		t.Fatalf("schema result: %+v", parsed)
	}
	keys := parsed["keys"].([]any)
	if len(keys) != 1 || keys[0] != "ID" {
		t.Fatalf("schema keys: %v", keys)
	}
	relations := parsed["relationshipFields"].([]any)
	if len(relations) != 1 || relations[0] != "ParentMapID" {
		t.Fatalf("schema relationship fields: %v", relations)
	}

	search, code := invoke(t, append([]string{"data", "db2", "search", "Map", "--field", "Name_lang", "--query", "dusk", "--limit", "10"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !search.OK || search.Context["snapshot"] != pin {
		t.Fatalf("search: %d %+v", code, search)
	}
	found := search.Result.(map[string]any)
	rows := found["rows"].([]any)
	if found["count"] != float64(1) || len(rows) != 1 || rows[0].(map[string]any)["Name_lang"] != "Duskwood" {
		t.Fatalf("search result: %+v", found)
	}

	foreign, code := invoke(t, append([]string{"data", "db2", "foreign-key", "Map", "--field", "ParentMapID", "--value", "1", "--limit", "10"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !foreign.OK {
		t.Fatalf("foreign-key: %d %+v", code, foreign)
	}
	matches := foreign.Result.(map[string]any)
	if matches["count"] != float64(2) || len(matches["rows"].([]any)) != 2 {
		t.Fatalf("foreign-key result: %+v", matches)
	}

	unknown, code := invoke(t, append([]string{"data", "db2", "search", "Map", "--field", "Nope", "--query", "x", "--limit", "10"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || unknown.Error.Code != "records.field_unknown" || unknown.Result != nil {
		t.Fatalf("unknown field: %d %+v", code, unknown)
	}
	mismatch, code := invoke(t, append([]string{"data", "db2", "foreign-key", "Map", "--field", "Name_lang", "--value", "1", "--limit", "10"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || mismatch.Error.Code != "records.field_type_mismatch" {
		t.Fatalf("string foreign key: %d %+v", code, mismatch)
	}
	for _, args := range [][]string{
		{"data", "db2", "search", "Map", "--field", "Name_lang", "--query", "dusk"},
		{"data", "db2", "foreign-key", "Map", "--field", "ParentMapID"},
		{"data", "db2", "foreign-key", "Map", "--value", "1", "--limit", "5"},
		{"data", "db2", "schema"},
		{"data", "db2", "schema", "Map", "Extra"},
		{"data", "db2", "schema", "Map", "--table", "Other"},
	} {
		result, code := invoke(t, append(args, dataArgs(pin, root, "--format=json")...)...)
		if code != 2 || result.OK || result.Result != nil {
			t.Fatalf("%v: %d %+v", args, code, result)
		}
	}
}

func TestDataDB2StreamCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	run := func(args ...string) ([]map[string]any, int) {
		t.Helper()
		var out, log bytes.Buffer
		code := Execute(context.Background(), args, &out, &log)
		if log.Len() != 0 {
			t.Fatalf("unexpected stderr: %s", log.String())
		}
		frames := []map[string]any{}
		decoder := json.NewDecoder(&out)
		for {
			var frame map[string]any
			if err := decoder.Decode(&frame); err != nil {
				// A refusal before any stream frame renders in a non-JSONL
				// format; anything after emitted frames must stay parseable.
				if !errors.Is(err, io.EOF) && len(frames) == 0 {
					break
				}
				if !errors.Is(err, io.EOF) {
					t.Fatalf("decode: %v: %s", err, out.String())
				}
				break
			}
			frames = append(frames, frame)
		}
		return frames, code
	}
	base := append([]string{"data", "db2", "stream", "Map"}, dataArgs(pin, root)...)
	frames, code := run(append(append([]string{}, base...), "--format=jsonl", "--limit", "10")...)
	if code != 0 || len(frames) != 5 {
		t.Fatalf("stream frames: %d %+v", code, frames)
	}
	begin, records0, end := frames[0], frames[1:4], frames[4]
	if begin["frame"] != "begin" || end["frame"] != "end" {
		t.Fatalf("frame order: %+v", frames)
	}
	beginBody := begin["begin"].(map[string]any)
	if beginBody["snapshot"] != pin || beginBody["table"] != "Map" {
		t.Fatalf("begin frame: %+v", begin)
	}
	for index, frame := range records0 {
		if frame["frame"] != "record" {
			t.Fatalf("frame %d is not a record: %+v", index, frame)
		}
		record := frame["record"].(map[string]any)
		if record["index"] != float64(index) {
			t.Fatalf("record index: %+v", record)
		}
	}
	endBody := end["end"].(map[string]any)
	if endBody["count"] != float64(3) || endBody["complete"] != true || endBody["truncated"] != false {
		t.Fatalf("end frame: %+v", end)
	}

	filtered, code := run(append(append([]string{}, base...), "--format=jsonl", "--limit", "10", "--filter", "ParentMapID=1")...)
	if code != 0 || len(filtered) != 4 {
		t.Fatalf("filtered stream: %d %+v", code, filtered)
	}
	if filtered[3]["end"].(map[string]any)["count"] != float64(2) {
		t.Fatalf("filtered end frame: %+v", filtered[3])
	}

	projected, code := run(append(append([]string{}, base...), "--format=jsonl", "--limit", "10", "--fields", "ID")...)
	if code != 0 {
		t.Fatalf("projected stream: %d", code)
	}
	first := projected[1]["record"].(map[string]any)["row"].(map[string]any)
	if len(first) != 1 || first["ID"] != float64(1) {
		t.Fatalf("projected row: %+v", first)
	}

	// Without --format jsonl the stream refuses to emit stream frames at all,
	// and a missing limit is a user-input failure before any stream frame is
	// written; failures still render their envelope error in the caller's
	// requested format.
	textFrames, code := run(append(append([]string{}, base...), "--format=text", "--limit", "10")...)
	if code != 2 || len(textFrames) != 0 {
		t.Fatalf("text stream: %d %+v", code, textFrames)
	}
	limited, code := run(append(append([]string{}, base...), "--format=jsonl")...)
	if code != 2 || streamStarted(limited) {
		t.Fatalf("missing limit: %d %+v", code, limited)
	}
	badField, code := run(append(append([]string{}, base...), "--format=jsonl", "--limit", "10", "--filter", "Nope=1")...)
	if code != 2 || streamStarted(badField) {
		t.Fatalf("unknown filter field: %d %+v", code, badField)
	}
}

// streamStarted reports whether any typed stream frame (frame field) reached
// stdout; envelope begin/error frames carry a type field instead.
func streamStarted(frames []map[string]any) bool {
	for _, frame := range frames {
		if frame["frame"] == "begin" || frame["frame"] == "record" || frame["frame"] == "end" {
			return true
		}
	}
	return false
}

func TestDataSpellVerbsCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	info, code := invoke(t, append([]string{"data", "spell", "info", "--spell-id", "100"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !info.OK || len(info.Captures) != 1 || info.Context["snapshot"] != pin {
		t.Fatalf("spell info: %d %+v", code, info)
	}
	resolved := info.Result.(map[string]any)
	if resolved["spellID"] != float64(100) || resolved["totalCount"] != float64(2) {
		t.Fatalf("spell info result: %+v", resolved)
	}
	spells := resolved["spells"].(map[string]any)
	if len(spells) != 2 {
		t.Fatalf("spell closure: %v", spells)
	}
	charge := spells["100"].(map[string]any)
	misc := charge["misc"].(map[string]any)
	if misc["castTime"] == nil || misc["castTime"].(map[string]any)["base"] != float64(1500) {
		t.Fatalf("spell misc: %+v", misc)
	}

	auras, code := invoke(t, append([]string{"data", "spell", "auras", "--spell-id", "200"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !auras.OK {
		t.Fatalf("spell auras: %d %+v", code, auras)
	}
	aura := auras.Result.(map[string]any)
	if fmt.Sprint(aura["hasAura"]) != "[200]" || fmt.Sprint(aura["noAura"]) != "[]" {
		t.Fatalf("aura result: %+v", aura)
	}

	summons, code := invoke(t, append([]string{"data", "spell", "summons", "--spell-id", "300", "--npc-id", "5000"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !summons.OK {
		t.Fatalf("spell summons: %d %+v", code, summons)
	}
	called := summons.Result.(map[string]any)
	if called["count"] != float64(1) || fmt.Sprint(called["summons"].([]any)[0].(map[string]any)["npcID"]) != "5000" {
		t.Fatalf("summons result: %+v", called)
	}
	other, code := invoke(t, append([]string{"data", "spell", "summons", "--spell-id", "300", "--npc-id", "9"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || other.Result.(map[string]any)["count"] != float64(0) {
		t.Fatalf("filtered summons: %d %+v", code, other)
	}

	for _, args := range [][]string{
		{"data", "spell", "info"},
		{"data", "spell", "auras"},
		{"data", "spell", "summons"},
		{"data", "spell", "info", "--spell-id", "0"},
		{"data", "spell", "info", "--max-depth", "-1"},
		{"data", "spell", "info", "--max-depth", "33"},
	} {
		result, code := invoke(t, append(args, dataArgs(pin, root, "--format=json")...)...)
		if code != 2 || result.OK || result.Result != nil {
			t.Fatalf("%v: %d %+v", args, code, result)
		}
	}
}

func TestDataItemVerbsCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	item, code := invoke(t, append([]string{"data", "item", "get", "--item-id", "9001"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !item.OK || len(item.Captures) != 1 {
		t.Fatalf("item get: %d %+v", code, item)
	}
	summary := item.Result.(map[string]any)["item"].(map[string]any)
	if summary["name"] != "Fixture Blade" || summary["inventoryType"] != float64(13) || summary["slotName"] != "One-Hand" || summary["classID"] != float64(2) {
		t.Fatalf("item summary: %+v", summary)
	}

	models, code := invoke(t, append([]string{"data", "item", "models", "--item-id", "9001", "--race-id", "4", "--gender", "1"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !models.OK {
		t.Fatalf("item models: %d %+v", code, models)
	}
	resolved := models.Result.(map[string]any)
	if resolved["displayID"] != float64(21) || fmt.Sprint(resolved["models"]) != "[61001]" || fmt.Sprint(resolved["textures"]) != "[71001]" || fmt.Sprint(resolved["geosetGroup"]) != "[1 2 3]" {
		t.Fatalf("item models result: %+v", resolved)
	}

	geosets, code := invoke(t, append([]string{"data", "item", "geosets", "--item-id", "9001"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !geosets.OK {
		t.Fatalf("item geosets: %d %+v", code, geosets)
	}
	geoset := geosets.Result.(map[string]any)
	if fmt.Sprint(geoset["helmetGeosetVis"]) != "[0 0 0]" || fmt.Sprint(geoset["helmetHide"]) != "[]" {
		t.Fatalf("geosets result: %+v", geoset)
	}

	textures, code := invoke(t, append([]string{"data", "item", "textures", "--item-id", "9001"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !textures.OK {
		t.Fatalf("item textures: %d %+v", code, textures)
	}
	sections := textures.Result.(map[string]any)["textures"].([]any)
	if len(sections) != 1 {
		t.Fatalf("textures result: %+v", textures.Result)
	}
	section := sections[0].(map[string]any)
	if section["section"] != float64(3) || section["fileDataID"] != float64(71001) {
		t.Fatalf("texture section: %+v", section)
	}

	missing, code := invoke(t, append([]string{"data", "item", "get", "--item-id", "999999"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 3 || missing.Error.Code != "records.record_not_found" || missing.Result != nil {
		t.Fatalf("missing item: %d %+v", code, missing)
	}
	for _, args := range [][]string{
		{"data", "item", "get"},
		{"data", "item", "models"},
		{"data", "item", "geosets", "--item-id", "0"},
		{"data", "item", "textures", "--item-id", "-1"},
	} {
		result, code := invoke(t, append(args, dataArgs(pin, root, "--format=json")...)...)
		if code != 2 || result.OK {
			t.Fatalf("%v: %d %+v", args, code, result)
		}
	}
}

func TestDataCreatureVerbsCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	display, code := invoke(t, append([]string{"data", "creature", "display", "--display-id", "400"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !display.OK || display.Context["snapshot"] != pin {
		t.Fatalf("creature display: %d %+v", code, display)
	}
	resolved := display.Result.(map[string]any)
	if resolved["modelID"] != float64(401) || resolved["fileDataID"] != float64(42000) || fmt.Sprint(resolved["textures"]) != "[41000]" || fmt.Sprint(resolved["variations"]) != "[101]" {
		t.Fatalf("creature display result: %+v", resolved)
	}

	byFile, code := invoke(t, append([]string{"data", "creature", "display", "--file-data-id", "42000"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !byFile.OK || byFile.Result.(map[string]any)["displayID"] != float64(400) {
		t.Fatalf("creature display by file: %d %+v", code, byFile)
	}

	model, code := invoke(t, append([]string{"data", "creature", "model", "--file-data-id", "42000"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !model.OK {
		t.Fatalf("creature model: %d %+v", code, model)
	}
	displays := model.Result.(map[string]any)["displays"].([]any)
	if len(displays) != 1 || displays[0].(map[string]any)["displayID"] != float64(400) {
		t.Fatalf("creature model result: %+v", model.Result)
	}

	both, code := invoke(t, append([]string{"data", "creature", "display", "--display-id", "400", "--file-data-id", "42000"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || both.Error.Code != "records.request_conflict" {
		t.Fatalf("both keys: %d %+v", code, both)
	}
	neither, code := invoke(t, append([]string{"data", "creature", "display"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || neither.OK {
		t.Fatalf("no key: %d %+v", code, neither)
	}
}

func TestDataEncounterAndDecorCLI(t *testing.T) {
	root, pin := dataCommandFixture(t)
	encounter, code := invoke(t, append([]string{"data", "encounter", "get", "--journal-encounter-id", "600"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !encounter.OK || len(encounter.Captures) != 2 || encounter.Context["snapshot"] != pin {
		t.Fatalf("encounter: %d %+v", code, encounter)
	}
	result := encounter.Result.(map[string]any)
	if result["journalEncounterID"] != float64(600) || result["sectionCount"] != float64(2) || fmt.Sprint(result["spellIDs"]) != "[700]" {
		t.Fatalf("encounter result: %+v", result)
	}
	sections := result["sections"].([]any)
	if len(sections) != 1 || len(sections[0].(map[string]any)["children"].([]any)) != 1 {
		t.Fatalf("section tree: %+v", sections)
	}
	related := result["relatedSpells"].([]any)
	if len(related) != 1 || related[0].(map[string]any)["name"] != "Fireball" {
		t.Fatalf("related spells: %+v", related)
	}
	if fmt.Sprint(result["unresolvedSectionIDs"]) != "[]" {
		t.Fatalf("unresolved sections: %+v", result)
	}
	for _, capture := range encounter.Captures {
		verified, exit := invoke(t, "evidence", "verify", capture.(map[string]any)["id"].(string), "--home", root, "--format=json")
		if exit != 0 || !verified.OK {
			t.Fatal(verified, exit)
		}
	}

	decor, code := invoke(t, append([]string{"data", "decor", "list", "--limit", "10"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !decor.OK {
		t.Fatalf("decor list: %d %+v", code, decor)
	}
	list := decor.Result.(map[string]any)
	if list["count"] != float64(2) || len(list["items"].([]any)) != 2 {
		t.Fatalf("decor list result: %+v", list)
	}

	byID, code := invoke(t, append([]string{"data", "decor", "get", "--id", "50"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !byID.OK || byID.Result.(map[string]any)["item"].(map[string]any)["name"] != "Fixture Lamp" {
		t.Fatalf("decor get by id: %d %+v", code, byID)
	}
	byItem, code := invoke(t, append([]string{"data", "decor", "get", "--item-id", "901"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !byItem.OK || byItem.Result.(map[string]any)["item"].(map[string]any)["id"] != float64(51) {
		t.Fatalf("decor get by item: %d %+v", code, byItem)
	}
	byModel, code := invoke(t, append([]string{"data", "decor", "get", "--model-file-data-id", "502"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 0 || !byModel.OK || byModel.Result.(map[string]any)["item"].(map[string]any)["id"] != float64(51) {
		t.Fatalf("decor get by model: %d %+v", code, byModel)
	}

	ambiguous, code := invoke(t, append([]string{"data", "decor", "get", "--id", "50", "--item-id", "901"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || ambiguous.Error.Code != "records.request_conflict" {
		t.Fatalf("decor conflict: %d %+v", code, ambiguous)
	}
	unbounded, code := invoke(t, append([]string{"data", "decor", "list"}, dataArgs(pin, root, "--format=json")...)...)
	if code != 2 || unbounded.OK {
		t.Fatalf("decor list without limit: %d %+v", code, unbounded)
	}
}

func TestDataVerbSourceSelectionRequired(t *testing.T) {
	for _, args := range [][]string{
		{"data", "db2", "schema", "Map", "--snapshot", "unused"},
		{"data", "db2", "schema", "Map", "--cdn"},
		{"data", "spell", "info", "--spell-id", "1", "--cdn"},
		{"data", "item", "get", "--item-id", "1"},
		{"data", "creature", "model", "--file-data-id", "1", "--snapshot", "unused"},
		{"data", "encounter", "get", "--journal-encounter-id", "1", "--cdn"},
		{"data", "decor", "get", "--id", "1", "--snapshot", "unused"},
	} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK || result.Error == nil || result.Error.Code != "command.invalid_arguments" {
			t.Fatalf("%v: %d %+v", args, code, result)
		}
	}
}
