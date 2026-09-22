package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

// Domain group "spell". The seed data mirrors the legacy golden oracle
// (wowdata fixtures/golden/go/spell/info-100-classic-era.json) at a size that
// can be checked by hand: Charge (100) triggers Charge Stun (7922) through a
// script effect, the description mentions "$7922d" (a four-digit reference
// that is deliberately NOT a spell reference), and the misc/period tables
// resolve exactly as the oracle shows.

func spellNameFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "SpellName", fileDataID: 301, identityExternal: true,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Name_lang", kind: 's', loc: true},
		},
		rows: rows,
	}
}

func spellFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "Spell", fileDataID: 302,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "Description_lang", kind: 's', loc: true},
			{name: "AuraDescription_lang", kind: 's', loc: true},
		},
		rows: rows,
	}
}

func spellEffectFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "SpellEffect", fileDataID: 303,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "SpellID", kind: 'i', bits: 32, foreign: "Spell::ID"},
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
		rows: rows,
	}
}

func spellMiscFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "SpellMisc", fileDataID: 304,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "SpellID", kind: 'i', bits: 32, foreign: "Spell::ID"},
			{name: "Attributes", kind: 'i', bits: 32, elements: 3},
			{name: "SchoolMask", kind: 'i', bits: 32, signed: true},
			{name: "Speed", kind: 'i', bits: 32, signed: true},
			{name: "SpellIconFileDataID", kind: 'i', bits: 32, signed: true},
			{name: "CastingTimeIndex", kind: 'i', bits: 32},
			{name: "DurationIndex", kind: 'i', bits: 32},
			{name: "RangeIndex", kind: 'i', bits: 32},
		},
		rows: rows,
	}
}

func spellPeriodFixtures() []fixtureTable {
	return []fixtureTable{
		{
			name: "SpellCastTimes", fileDataID: 305,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Base", kind: 'i', bits: 32, signed: true},
				{name: "Minimum", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(1), int64(0), int64(0)}},
		},
		{
			name: "SpellDuration", fileDataID: 306,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "Duration", kind: 'i', bits: 32, signed: true},
				{name: "MaxDuration", kind: 'i', bits: 32, signed: true},
			},
			rows: [][]any{{uint64(2), int64(1000), int64(1000)}},
		},
		{
			name: "SpellRange", fileDataID: 307,
			columns: []fixtureColumn{
				{name: "ID", kind: 'i', bits: 32, identity: true},
				{name: "DisplayName_lang", kind: 's', loc: true},
				{name: "RangeMin", kind: 'i', bits: 32, signed: true, elements: 2},
				{name: "RangeMax", kind: 'i', bits: 32, signed: true, elements: 2},
			},
			rows: [][]any{{uint64(1), "Charge", []int64{8, 8}, []int64{25, 25}}},
		},
	}
}

func chargeFixtures() []fixtureTable {
	tables := []fixtureTable{
		spellNameFixture([][]any{{uint64(100), "Charge"}, {uint64(7922), "Charge Stun"}}),
		spellFixture([][]any{
			{uint64(100), "Charge an enemy, generate $/10;s2 rage, and stun it for $7922d.", ""},
			{uint64(7922), "", "Stunned."},
		}),
		spellEffectFixture([][]any{
			{uint64(1), uint64(100), uint64(0), int64(96), int64(0), uint64(0), uint64(0), float32(0), int64(0), []int64{6, 0}, []int64{0, 0}, []int64{0, 0}},
			{uint64(2), uint64(100), uint64(2), int64(64), int64(0), uint64(7922), uint64(0), float32(0), int64(0), []int64{6, 0}, []int64{0, 0}, []int64{0, 0}},
			{uint64(3), uint64(7922), uint64(0), int64(6), int64(12), uint64(0), uint64(180000), float32(-1000.5), int64(0), []int64{6, 0}, []int64{0, 0}, []int64{0, 0}},
		}),
		spellMiscFixture([][]any{
			{uint64(1), uint64(100), []int64{805634064, 1024, 0}, int64(1), int64(0), int64(132337), uint64(1), uint64(0), uint64(1)},
			{uint64(2), uint64(7922), []int64{327696, 512, 0}, int64(1), int64(0), int64(135860), uint64(1), uint64(2), uint64(1)},
		}),
	}
	return append(tables, spellPeriodFixtures()...)
}

func TestSpellInfoTraversesTriggersAndReferences(t *testing.T) {
	fx := newFixture(t, chargeFixtures())
	reading, err := records.InspectSpellInfo(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellInfoRequest{SpellID: 100})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.SeedCount != 1 || result.TotalCount != 2 || result.ChainDepth != 2 {
		t.Fatalf("chain = %+v", result)
	}
	if len(result.Triggers) != 1 || len(result.Triggers[100]) != 1 || result.Triggers[100][0] != 7922 {
		t.Fatalf("triggers = %v", result.Triggers)
	}
	if len(result.DescRefs) != 0 {
		t.Fatalf("a four-digit $7922d mention is not a spell reference: %v", result.DescRefs)
	}
	charge := result.Spells[100]
	if charge.Name == nil || *charge.Name != "Charge" || charge.Description == nil || charge.AuraDescription != nil {
		t.Fatalf("charge presence/absence = %+v", charge)
	}
	if !charge.IsSeed || len(charge.TriggeredBy) != 0 {
		t.Fatalf("charge seed = %+v", charge)
	}
	stun := result.Spells[7922]
	if stun.Name == nil || *stun.Name != "Charge Stun" || stun.Description != nil || stun.AuraDescription == nil || *stun.AuraDescription != "Stunned." {
		t.Fatalf("stun presence/absence = %+v", stun)
	}
	if stun.IsSeed || len(stun.TriggeredBy) != 1 || stun.TriggeredBy[0] != 100 {
		t.Fatalf("stun provenance = %+v", stun)
	}
	misc := charge.Misc
	if misc == nil || misc.SchoolMask != 1 || misc.SpellIconFileDataID != 132337 {
		t.Fatalf("misc = %+v", misc)
	}
	if misc.CastTime == nil || misc.CastTime.Base != 0 || misc.Duration != nil || misc.Range == nil ||
		misc.Range.DisplayName != "Charge" || len(misc.Range.RangeMin) != 2 || misc.Range.RangeMax[1] != 25 {
		t.Fatalf("misc periods = %+v", misc)
	}
	if stun.Misc == nil || stun.Misc.Duration == nil || stun.Misc.Duration.Duration != 1000 || stun.Misc.Duration.MaxDuration != 1000 {
		t.Fatalf("stun duration = %+v", stun.Misc)
	}
	if len(charge.Effects) != 2 || charge.Effects[1].Effect != 64 || charge.Effects[1].EffectTriggerSpell == nil || *charge.Effects[1].EffectTriggerSpell != 7922 {
		t.Fatalf("charge effects = %+v", charge.Effects)
	}
	if charge.Effects[0].EffectTriggerSpell != nil {
		t.Fatalf("zero trigger must be absent: %+v", charge.Effects[0])
	}
	// EffectBasePoints comes from the integral float column (legacy semantics).
	if stun.Effects[0].EffectBasePoints != -1000 || stun.Effects[0].EffectAura != 12 || stun.Effects[0].EffectAuraPeriod == nil || *stun.Effects[0].EffectAuraPeriod != 180000 {
		t.Fatalf("stun effect = %+v", stun.Effects[0])
	}
	links := result.Relations
	if len(links) < 3 {
		t.Fatalf("relation provenance missing: %+v", links)
	}
	found := false
	for _, link := range links {
		if link.FromTable == "SpellEffect" && link.FromField == "EffectTriggerSpell" && link.FromID == 2 && link.ToKind == "spell" && link.ToID == 7922 {
			found = true
		}
	}
	if !found {
		t.Fatalf("trigger link provenance: %+v", links)
	}
}

func TestSpellInfoDepthLimitIsHonest(t *testing.T) {
	fx := newFixture(t, chargeFixtures())
	reading, err := records.InspectSpellInfo(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellInfoRequest{SpellID: 100, MaxDepth: 1})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.ChainDepth != 1 || result.TotalCount != 2 {
		t.Fatalf("depth bound = %+v", result)
	}
	if !result.Truncated || result.Complete || result.TruncationReason != "max_depth" {
		t.Fatalf("depth truncation must be visible: %+v", result.DataContext)
	}
	// The known leaf still carries full detail (intentional upgrade over the
	// legacy loader, which left depth-limit leaves half-populated).
	if result.Spells[7922].Description != nil && result.Spells[7922].AuraDescription == nil {
		t.Fatalf("leaf detail lost: %+v", result.Spells[7922])
	}
	if _, err := records.InspectSpellInfo(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellInfoRequest{SpellID: 100, MaxDepth: records.MaxSpellDepth + 1}); !errors.Is(err, records.ErrQueryLimit) {
		t.Fatalf("unbounded depth must be rejected, got %v", err)
	}
}

func TestSpellInfoHandlesCyclesAndDescriptionReferences(t *testing.T) {
	tables := []fixtureTable{
		spellNameFixture([][]any{{uint64(10), "A"}, {uint64(20), "B"}}),
		spellFixture([][]any{
			{uint64(10), "see $12345x and $@spellname77", ""},
			{uint64(20), "", "from $@spellname10"},
		}),
		spellEffectFixture([][]any{
			{uint64(1), uint64(10), uint64(0), int64(64), int64(0), uint64(20), uint64(0), float32(0), int64(0), []int64{0, 0}, []int64{0, 0}, []int64{0, 0}},
			{uint64(2), uint64(20), uint64(0), int64(64), int64(0), uint64(10), uint64(0), float32(0), int64(0), []int64{0, 0}, []int64{0, 0}, []int64{0, 0}},
		}),
		spellMiscFixture([][]any{}),
	}
	tables = append(tables, spellPeriodFixtures()...)
	fx := newFixture(t, tables)
	reading, err := records.InspectSpellInfo(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellInfoRequest{SpellID: 10})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	// A -> B -> A terminates through the visited set; the cycle stays visible.
	if result.TotalCount != 4 || result.Triggers[10][0] != 20 || result.Triggers[20][0] != 10 {
		t.Fatalf("cycle handling = %+v", result)
	}
	refs := result.DescRefs[10]
	if len(refs) != 2 || refs[0] != 12345 || refs[1] != 77 {
		t.Fatalf("description references = %v", result.DescRefs)
	}
	if result.DescRefs[20][0] != 10 {
		t.Fatalf("aura references = %v", result.DescRefs)
	}
	if _, ok := result.Spells[12345]; !ok {
		t.Fatal("referenced spell must be collected")
	}
	missing := result.Spells[12345]
	if missing.Name != nil || missing.Misc != nil || len(missing.Effects) != 0 {
		t.Fatalf("absent rows must stay nil/empty: %+v", missing)
	}
	if !result.Complete || result.Truncated {
		t.Fatalf("a cycle is handled data, not truncation: %+v", result.DataContext)
	}
}

// Cross-check against the legacy auras oracle
// (wowdata fixtures/golden/go/spell/auras-spell-1-classic-era.json): the spell
// appears in exactly one of hasAura / noAura.
func TestSpellAurasPresenceSemantics(t *testing.T) {
	tables := []fixtureTable{spellEffectFixture([][]any{
		{uint64(1), uint64(1), uint64(0), int64(252), int64(0), uint64(0), uint64(0), float32(0), int64(0), []int64{1, 9}, []int64{0, 0}, []int64{0, 0}},
		{uint64(2), uint64(7922), uint64(0), int64(6), int64(12), uint64(0), uint64(0), float32(0), int64(0), []int64{6, 0}, []int64{0, 0}, []int64{0, 0}},
	})}
	fx := newFixture(t, tables)
	absent, err := records.InspectSpellAuras(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellAurasRequest{SpellID: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(absent.Result.HasAura) != 0 || len(absent.Result.NoAura) != 1 || absent.Result.NoAura[0] != 1 {
		t.Fatalf("no-aura case = %+v", absent.Result)
	}
	present, err := records.InspectSpellAuras(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellAurasRequest{SpellID: 7922})
	if err != nil {
		t.Fatal(err)
	}
	if len(present.Result.HasAura) != 1 || present.Result.HasAura[0] != 7922 || len(present.Result.NoAura) != 0 {
		t.Fatalf("has-aura case = %+v", present.Result)
	}
}

// Cross-check against the legacy summons oracle
// (wowdata fixtures/golden/go/spell/summons-513-npc-329-classic-era.json):
// summon effects carry their NPC in EffectMiscValue[0] and honour the NPC
// filter.
func TestSpellSummonsSemantics(t *testing.T) {
	tables := []fixtureTable{spellEffectFixture([][]any{
		{uint64(678946), uint64(513), uint64(0), int64(28), int64(0), uint64(0), uint64(0), float32(0), int64(0), []int64{18, 0}, []int64{0, 0}, []int64{329, 67}},
		{uint64(1), uint64(513), uint64(1), int64(6), int64(12), uint64(0), uint64(0), float32(0), int64(0), []int64{6, 0}, []int64{0, 0}, []int64{0, 0}},
	})}
	fx := newFixture(t, tables)
	reading, err := records.InspectSpellSummons(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellSummonsRequest{SpellID: 513, NPCID: 329})
	if err != nil {
		t.Fatal(err)
	}
	if reading.Result.Count != 1 {
		t.Fatalf("summons = %+v", reading.Result)
	}
	entry := reading.Result.Summons[0]
	if entry.SpellID != 513 || entry.NPCID != 329 || entry.EffectIndex != 0 {
		t.Fatalf("summon = %+v", entry)
	}
	if row := entry.Row; row["ID"] != uint64(678946) || row["SpellID"] != uint64(513) {
		t.Fatalf("raw effect row = %v", row)
	}
	all, err := records.InspectSpellSummons(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellSummonsRequest{SpellID: 513})
	if err != nil {
		t.Fatal(err)
	}
	if all.Result.Count != 1 || all.Result.Summons[0].NPCID != 329 {
		t.Fatalf("unfiltered summons = %+v", all.Result)
	}
	other, err := records.InspectSpellSummons(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.SpellSummonsRequest{SpellID: 513, NPCID: 999})
	if err != nil {
		t.Fatal(err)
	}
	if other.Result.Count != 0 || len(other.Result.Summons) != 0 {
		t.Fatalf("filtered summons = %+v", other.Result)
	}
}
