package records

import (
	"context"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

func TestPinnedSemanticMultiwordName(t *testing.T) {
	// WoWDBDefs 541f07fd4bf0e3ade0b6bafcadc1343dd699adb2, meta/enums/ChrRaceID.dbde.
	rows, err := parseSemanticValues(context.Background(), []byte("4 Night Elf\n34 Dark Iron Dwarf\n"), "12.1.5.70688")
	want := [][]any{{uint64(4), "Night Elf", ""}, {uint64(34), "Dark Iron Dwarf", ""}}
	if err != nil || !reflect.DeepEqual(rows, want) {
		t.Fatalf("rows = %#v, err = %v; want %#v", rows, err, want)
	}
}

func TestSemanticNameCommentAndBudget(t *testing.T) {
	// Names and comments are independently optional in the pinned meta format.
	raw := []byte("4 Night  Elf?//adjacent comment\n5 // unnamed\n6\n")
	rows, err := parseSemanticValues(context.Background(), raw, "12.1.5.70688")
	want := [][]any{{uint64(4), "Night  Elf?", "adjacent comment"}, {uint64(5), nil, "unnamed"}, {uint64(6), nil, ""}}
	if err != nil || !reflect.DeepEqual(rows, want) {
		t.Fatalf("rows=%#v err=%v", rows, err)
	}
	limits := resource.DefaultLimits()
	limits.MetadataBytes = int64(len(raw)) - 1
	budget := resource.New(limits)
	if _, err := parseSemanticValues(context.Background(), raw, "12.1.5.70688", budget); err == nil || budget.Snapshot().DecodeWork != 0 {
		t.Fatalf("raw metadata must be charged before parsing: %v %+v", err, budget.Snapshot())
	}
}

func TestSemanticBuildPresetBoundaries(t *testing.T) {
	for _, tc := range []struct {
		build string
		want  bool
	}{
		{"3.0.0.8119", false}, {"3.0.0.8120", true}, {"3.3.5.12340", true}, {"3.3.5.12341", false}, {"3.4.3.51505", false},
	} {
		got, err := matchesSemanticBuild(tc.build, "WotLK")
		if err != nil || got != tc.want {
			t.Errorf("%s: %v %v; want %v", tc.build, got, err, tc.want)
		}
	}
	if _, err := matchesSemanticBuild("12.1.5.70688", "UnknownExpansion"); err == nil {
		t.Fatal("unknown preset accepted")
	}
}

func TestPinnedSemanticBuildOverrides(t *testing.T) {
	// SpellEffect.dbde and WeatherType.dbde at the same pinned upstream commit.
	for _, tc := range []struct{ raw, build, name string }{
		{"(BUILD 3.0.1.8303-3.3.5.12340) 146 ACTIVATE_RUNE\n146 RESTORE_GARRISON_TROOP_VITALITY\n", "3.3.5.12340", "ACTIVATE_RUNE"},
		{"146 RESTORE_GARRISON_TROOP_VITALITY\n(BUILD 3.0.1.8303-3.3.5.12340) 146 ACTIVATE_RUNE\n", "3.3.5.12340", "ACTIVATE_RUNE"},
		{"(BUILD TBC, WotLK, Cata, MoP, WoD, Legion, BfA, SL, DF, 11.0.0.54311-11.1.7.61967) 0 None\n0 Clear\n", "12.1.5.70688", "Clear"},
		{"(BUILD TBC, WotLK, Cata, MoP, WoD, Legion, BfA, SL, DF, 11.0.0.54311-11.1.7.61967) 0 None\n0 Clear\n", "3.3.5.12340", "None"},
	} {
		rows, err := parseSemanticValues(context.Background(), []byte(tc.raw), tc.build)
		if err != nil || len(rows) != 1 || rows[0][1] != tc.name {
			t.Errorf("build %s: rows=%#v err=%v; want only %s", tc.build, rows, err, tc.name)
		}
	}
}

func TestPinnedSemanticSyntaxBuildsAndConditions(t *testing.T) {
	mappings, err := parseSemanticMappings([]byte("FLAGS SpellMisc::Attributes[0] SpellAttributes0\nENUM SpellEffect::EffectMiscValue[0] SpellModOp SpellEffect::EffectAura=107\n"), "SpellEffect")
	if err != nil || len(mappings) != 1 || mappings[0].conditionField != "SpellEffect::EffectAura" || mappings[0].conditionValue != "107" {
		t.Fatalf("%+v %v", mappings, err)
	}
	raw := []byte("0x8000000000000000 HIGH // high bit\n(BUILD 3.0.1.8303-3.3.5.12340, 4.0.0.11792) 146 OLD\n(BUILD 12.1.0.69933) 146 CURRENT\n5\n-1 NONE\n")
	rows, err := parseSemanticValues(context.Background(), raw, "12.1.0.69933")
	if err != nil || len(rows) != 4 || rows[0][0] != uint64(1)<<63 || rows[1][1] != "CURRENT" || rows[2][1] != nil || rows[3][0] != int64(-1) {
		t.Fatalf("%+v %v", rows, err)
	}
	if _, err := parseSemanticValues(context.Background(), []byte("(BOGUS 1) 2 UNKNOWN"), "12.1.0.69933"); err == nil {
		t.Fatal("unknown applicability accepted")
	}
	if _, err := parseSemanticMappings([]byte("FLAGS SpellMisc::Attributes ../../escape"), "SpellMisc"); err == nil {
		t.Fatal("unsafe metadata name")
	}
}
