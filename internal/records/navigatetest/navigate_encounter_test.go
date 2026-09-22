package navigatetest

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

// Domain group "encounter". The tree mirrors the legacy golden oracle
// (wowdata fixtures/golden/go/encounter/get-132-retail.json): root sections
// ordered by (OrderIndex, ID) with structural NextSibling chains, children
// built from the FirstChild chain. Related spells and their icon file IDs are
// carried so asset exports can follow (AST-04 split workflow).

func encounterSectionFixture(rows [][]any) fixtureTable {
	return fixtureTable{
		name: "JournalEncounterSection", fileDataID: 331, identityExternal: true,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "JournalEncounterID", kind: 'i', bits: 32, foreign: "JournalEncounter::ID"},
			{name: "Title_lang", kind: 's', loc: true},
			{name: "BodyText_lang", kind: 's', loc: true},
			{name: "SpellID", kind: 'i', bits: 32, foreign: "Spell::ID"},
			{name: "IconFlags", kind: 'i', bits: 32, signed: true},
			{name: "Type", kind: 'i', bits: 32, signed: true},
			{name: "DifficultyMask", kind: 'i', bits: 32, signed: true},
			{name: "IconCreatureDisplayInfoID", kind: 'i', bits: 32},
			{name: "OrderIndex", kind: 'i', bits: 32},
			{name: "ParentSectionID", kind: 'i', bits: 32},
			{name: "FirstChildSectionID", kind: 'i', bits: 32},
			{name: "NextSiblingSectionID", kind: 'i', bits: 32},
		},
		rows: rows,
	}
}

func encounterFixtures(rows [][]any) []fixtureTable {
	return []fixtureTable{
		encounterSectionFixture(rows),
		spellNameFixture([][]any{{uint64(123), "Personal Phalanx"}}),
		spellMiscFixture([][]any{
			{uint64(1), uint64(123), []int64{0, 0, 0}, int64(1), int64(0), int64(132337), uint64(0), uint64(0), uint64(0)},
		}),
	}
}

func encounterRows() [][]any {
	return [][]any{
		{uint64(816), uint64(132), "Mighty Stomp", "Throngus stomps his foot.", uint64(0), int64(0), int64(2), int64(-1), uint64(0), uint64(1), uint64(0), uint64(0), uint64(817)},
		{uint64(817), uint64(132), "Cave In", "Stones fall in a $74986A1 yard area.", uint64(0), int64(0), int64(2), int64(-1), uint64(0), uint64(2), uint64(0), uint64(0), uint64(818)},
		{uint64(818), uint64(132), "Pick Weapon", "Choose a weapon.", uint64(0), int64(0), int64(1), int64(-1), uint64(0), uint64(3), uint64(0), uint64(0), uint64(0)},
		{uint64(819), uint64(132), "Shield Abilities", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(4), uint64(0), uint64(820), uint64(0)},
		{uint64(820), uint64(132), "Personal Phalanx", "Damage reduced.", uint64(123), int64(2), int64(2), int64(-1), uint64(0), uint64(5), uint64(819), uint64(0), uint64(821)},
		{uint64(821), uint64(132), "Void Droplet", "", uint64(0), int64(1), int64(1), int64(0), uint64(94300), uint64(6), uint64(819), uint64(0), uint64(0)},
	}
}

// Cross-check against the legacy encounter oracle
// (wowdata fixtures/golden/go/encounter/get-132-retail.json) for tree shape,
// ordering and per-section spell IDs.
func TestEncounterBuildsSectionTreeWithRelations(t *testing.T) {
	fx := newFixture(t, encounterFixtures(encounterRows()))
	reading, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 132})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.JournalEncounterID != 132 || result.SectionCount != 6 || result.SpellCount != 1 {
		t.Fatalf("encounter = %+v", result)
	}
	if len(result.SpellIDs) != 1 || result.SpellIDs[0] != 123 {
		t.Fatalf("spell ids = %v", result.SpellIDs)
	}
	if len(result.Sections) != 4 {
		t.Fatalf("roots = %d", len(result.Sections))
	}
	first := result.Sections[0]
	if first.ID != 816 || first.Title != "Mighty Stomp" || first.Type != 2 || first.DifficultyMask != -1 ||
		first.OrderIndex != 1 || first.ParentSectionID != 0 || first.FirstChildSectionID != 0 || first.NextSiblingSectionID != 817 {
		t.Fatalf("root section = %+v", first)
	}
	if result.Sections[1].ID != 817 || result.Sections[2].ID != 818 || result.Sections[3].ID != 819 {
		t.Fatalf("root order = %v %v %v %v", result.Sections[0].ID, result.Sections[1].ID, result.Sections[2].ID, result.Sections[3].ID)
	}
	abilities := result.Sections[3]
	if len(abilities.Children) != 2 || abilities.Children[0].ID != 820 || abilities.Children[1].ID != 821 {
		t.Fatalf("child chain = %+v", abilities.Children)
	}
	if len(abilities.Children[0].SpellIDs) != 1 || abilities.Children[0].SpellIDs[0] != 123 {
		t.Fatalf("section spells = %+v", abilities.Children[0])
	}
	if abilities.Children[1].IconCreatureDisplayInfoID != 94300 {
		t.Fatalf("creature icon = %+v", abilities.Children[1])
	}
	// Related spells resolve name and icon file ID with provenance.
	if len(result.RelatedSpells) != 1 {
		t.Fatalf("related spells = %+v", result.RelatedSpells)
	}
	spell := result.RelatedSpells[0]
	if spell.SpellID != 123 || spell.Name == nil || *spell.Name != "Personal Phalanx" || spell.IconFileDataID != 132337 {
		t.Fatalf("related spell = %+v", spell)
	}
	if len(result.RelatedFiles) != 1 || result.RelatedFiles[0].Kind != "icon_file" ||
		result.RelatedFiles[0].FileDataID != 132337 || result.RelatedFiles[0].OwnerID != 123 {
		t.Fatalf("related files = %+v", result.RelatedFiles)
	}
	// AST-04: one result capture plus a relation manifest capture keeps the
	// split workflow traceable to this single request.
	if len(reading.Captures) != 2 {
		t.Fatalf("captures = %+v", reading.Captures)
	}
	found := false
	for _, link := range result.Relations {
		if link.FromTable == "JournalEncounterSection" && link.FromField == "SpellID" && link.FromID == 820 && link.ToKind == "spell" && link.ToID == 123 {
			found = true
		}
	}
	if !found {
		t.Fatalf("section->spell provenance: %+v", result.Relations)
	}
	if !result.Complete || result.Truncated || result.Partial {
		t.Fatalf("honesty flags = %+v", result.DataContext)
	}
}

// Cross-check against the legacy empty oracle
// (wowdata fixtures/golden/go/encounter/get-1-classic-era.json): no sections is
// an empty, complete answer that does not demand the spell tables.
func TestEncounterEmptyMatchesLegacyOracle(t *testing.T) {
	fx := newFixture(t, []fixtureTable{encounterSectionFixture([][]any{})})
	reading, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 1})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.SectionCount != 0 || result.SpellCount != 0 || len(result.SpellIDs) != 0 || len(result.Sections) != 0 {
		t.Fatalf("empty encounter = %+v", result)
	}
	if !result.Complete {
		t.Fatalf("empty is complete: %+v", result.DataContext)
	}
}

func TestEncounterRejectsStructuralCycles(t *testing.T) {
	rows := [][]any{
		{uint64(900), uint64(1), "A", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(1), uint64(0), uint64(901), uint64(0)},
		{uint64(901), uint64(1), "B", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(2), uint64(900), uint64(900), uint64(0)},
	}
	fx := newFixture(t, []fixtureTable{encounterSectionFixture(rows)})
	_, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 1})
	if !errors.Is(err, records.ErrRelationCycle) {
		t.Fatalf("a structural cycle must fail honestly, got %v", err)
	}
	// A sibling chain that repeats a node is the same class of failure.
	loop := [][]any{
		{uint64(910), uint64(2), "A", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(1), uint64(0), uint64(911), uint64(0)},
		{uint64(911), uint64(2), "B", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(2), uint64(910), uint64(0), uint64(911)},
	}
	fx = newFixture(t, []fixtureTable{encounterSectionFixture(loop)})
	if _, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 2}); !errors.Is(err, records.ErrRelationCycle) {
		t.Fatalf("a sibling loop must fail honestly, got %v", err)
	}
}

func TestEncounterDepthAndUnresolvedReporting(t *testing.T) {
	rows := append(encounterRows(),
		[]any{uint64(822), uint64(132), "Grandchild", "", uint64(0), int64(0), int64(2), int64(-1), uint64(0), uint64(7), uint64(820), uint64(0), uint64(0)})
	fx := newFixture(t, encounterFixtures(rows))
	depth, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 132, MaxDepth: 1})
	if err != nil {
		t.Fatal(err)
	}
	if !depth.Result.Truncated || depth.Result.Complete || depth.Result.TruncationReason != "max_depth" {
		t.Fatalf("depth cut = %+v", depth.Result.DataContext)
	}
	if len(depth.Result.Sections[3].Children) != 2 {
		t.Fatalf("children stay within depth: %+v", depth.Result.Sections[3])
	}
	if len(depth.Result.Sections[3].Children[0].Children) != 0 {
		t.Fatalf("grandchildren must be cut: %+v", depth.Result.Sections[3].Children[0])
	}
	if len(depth.Result.UnresolvedSectionIDs) != 1 || depth.Result.UnresolvedSectionIDs[0] != 822 {
		t.Fatalf("cut children must be listed: %v", depth.Result.UnresolvedSectionIDs)
	}
	// Orphaned rows (parent outside the encounter) are reported, not dropped.
	orphan := append(encounterRows(),
		[]any{uint64(850), uint64(132), "Orphan", "", uint64(0), int64(0), int64(0), int64(-1), uint64(0), uint64(9), uint64(7777), uint64(0), uint64(0)})
	fx = newFixture(t, encounterFixtures(orphan))
	report, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 132})
	if err != nil {
		t.Fatal(err)
	}
	if report.Result.SectionCount != 7 || !report.Result.Partial {
		t.Fatalf("orphan report = %+v", report.Result)
	}
	found := false
	for _, id := range report.Result.UnresolvedSectionIDs {
		if id == 850 {
			found = true
		}
	}
	if !found {
		t.Fatalf("orphan id must be listed: %v", report.Result.UnresolvedSectionIDs)
	}
}
