package records

import (
	"context"
	"slices"

	"github.com/follenfang/lycheedev/internal/evidence"
)

// Domain group: encounter. The section tree is rebuilt from the structural
// Parent/FirstChild/NextSibling links with cycle detection (a structural cycle
// is an honest records.relation_cycle failure, never a hang), a depth bound
// and explicit unresolved/truncated reporting. Related spell IDs and their
// icon file IDs are carried so a caller can request asset exports afterwards;
// no image export happens here (AST-04 split workflow).

// EncounterGetRequest selects one journal encounter.
type EncounterGetRequest struct {
	JournalEncounterID uint32 `json:"journalEncounterID"`
	MaxDepth           int    `json:"maxDepth"`
}

type EncounterSection struct {
	ID                        uint32              `json:"id"`
	Title                     string              `json:"title"`
	BodyText                  string              `json:"bodyText"`
	SpellID                   uint32              `json:"spellID"`
	IconFlags                 int                 `json:"iconFlags"`
	Type                      int                 `json:"type"`
	DifficultyMask            int                 `json:"difficultyMask"`
	IconCreatureDisplayInfoID uint32              `json:"iconCreatureDisplayInfoID"`
	OrderIndex                uint32              `json:"orderIndex"`
	ParentSectionID           uint32              `json:"parentSectionID"`
	FirstChildSectionID       uint32              `json:"firstChildSectionID"`
	NextSiblingSectionID      uint32              `json:"nextSiblingSectionID"`
	Children                  []*EncounterSection `json:"children,omitempty"`
	SpellIDs                  []uint32            `json:"spellIDs,omitempty"`
}

// RelatedSpell is one spell referenced by the encounter's sections.
type RelatedSpell struct {
	SpellID        uint32  `json:"spellID"`
	Name           *string `json:"name"`
	IconFileDataID uint32  `json:"iconFileDataID"`
}

// EncounterRelatedFile is one asset file a caller can export afterwards.
type EncounterRelatedFile struct {
	Kind       string `json:"kind"`
	FileDataID uint32 `json:"fileDataID"`
	OwnerKind  string `json:"ownerKind"`
	OwnerID    uint32 `json:"ownerID"`
}

type EncounterGetResult struct {
	DataContext
	JournalEncounterID   uint32                 `json:"journalEncounterID"`
	SectionCount         int                    `json:"sectionCount"`
	SpellCount           int                    `json:"spellCount"`
	SpellIDs             []uint32               `json:"spellIDs"`
	Sections             []*EncounterSection    `json:"sections"`
	RelatedSpells        []RelatedSpell         `json:"relatedSpells"`
	RelatedFiles         []EncounterRelatedFile `json:"relatedFiles"`
	UnresolvedSectionIDs []uint32               `json:"unresolvedSectionIDs"`
}

// RelationManifest records every relation one request discovered, so a later
// asset export set can trace each file back to the single request that named
// it (the split replacement for the legacy combined export).
type RelationManifest struct {
	Operation        string                 `json:"operation"`
	Root             string                 `json:"root"`
	Snapshot         string                 `json:"snapshot"`
	DataBuild        string                 `json:"dataBuild"`
	Relations        []RelationLink         `json:"relations"`
	RelatedFiles     []EncounterRelatedFile `json:"relatedFiles"`
	Complete         bool                   `json:"complete"`
	Truncated        bool                   `json:"truncated"`
	TruncationReason string                 `json:"truncationReason"`
}

type sectionNode struct {
	row   map[string]any
	id    uint32
	order uint32
}

// InspectEncounter builds the bounded section tree with related spell IDs and
// the relation manifest for one journal encounter.
func InspectEncounter(ctx context.Context, root, snapshot string, query FileQuery, request EncounterGetRequest) (Reading[EncounterGetResult], error) {
	maxDepth := request.MaxDepth
	if maxDepth <= 0 {
		maxDepth = MaxEncounterDepth
	}
	if maxDepth > MaxEncounterDepth {
		return Reading[EncounterGetResult]{}, ErrQueryLimit
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[EncounterGetResult], error) {
		sectionsTable, err := nav.open(ctx, "JournalEncounterSection")
		if err != nil {
			return Reading[EncounterGetResult]{}, err
		}
		collected, extra, err := nav.rowsByField(ctx, sectionsTable, "JournalEncounterID", request.JournalEncounterID, MaxEncounterSection)
		if err != nil {
			return Reading[EncounterGetResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		nodes := map[uint32]*sectionNode{}
		for _, row := range collected {
			id := rowUint32(row.row, "ID")
			if id == 0 {
				nav.addPartial(PartialRowMissing)
				continue
			}
			if _, exists := nodes[id]; !exists {
				nodes[id] = &sectionNode{row: row.row, id: id, order: uint32(rowInt(row.row, "OrderIndex"))}
			}
		}
		unresolved := []uint32{}
		noteUnresolved := func(id uint32) {
			if id != 0 && !slices.Contains(unresolved, id) {
				unresolved = append(unresolved, id)
				nav.addPartial(PartialRelationUnresolved)
			}
		}
		roots := []*sectionNode{}
		for _, node := range sortedNodes(nodes) {
			if rowUint32(node.row, "ParentSectionID") == 0 {
				roots = append(roots, node)
			}
		}
		sortSections(roots)
		visited := map[uint32]bool{}
		var build func(node *sectionNode, depth int) (*EncounterSection, error)
		build = func(node *sectionNode, depth int) (*EncounterSection, error) {
			if visited[node.id] {
				return nil, ErrRelationCycle
			}
			visited[node.id] = true
			defer delete(visited, node.id)
			section := &EncounterSection{
				ID:    node.id,
				Title: rowString(node.row, "Title_lang"), BodyText: rowString(node.row, "BodyText_lang"),
				SpellID: rowUint32(node.row, "SpellID"), IconFlags: rowInt(node.row, "IconFlags"),
				Type: rowInt(node.row, "Type"), DifficultyMask: rowInt(node.row, "DifficultyMask"),
				IconCreatureDisplayInfoID: rowUint32(node.row, "IconCreatureDisplayInfoID"),
				OrderIndex:                node.order,
				ParentSectionID:           rowUint32(node.row, "ParentSectionID"),
				FirstChildSectionID:       rowUint32(node.row, "FirstChildSectionID"),
				NextSiblingSectionID:      rowUint32(node.row, "NextSiblingSectionID"),
			}
			if section.SpellID != 0 {
				section.SpellIDs = []uint32{section.SpellID}
				nav.link("JournalEncounterSection", "SpellID", uint64(node.id), "spell", uint64(section.SpellID), depth)
			}
			if section.IconCreatureDisplayInfoID != 0 {
				nav.link("JournalEncounterSection", "IconCreatureDisplayInfoID", uint64(node.id), "creature_display", uint64(section.IconCreatureDisplayInfoID), depth)
			}
			// Children follow the structural FirstChild/NextSibling chain when
			// present; otherwise the explicit parent link, sorted stably.
			if section.FirstChildSectionID != 0 {
				chain := []uint32{}
				seen := map[uint32]bool{}
				field := "FirstChildSectionID"
				cursor := section.FirstChildSectionID
				for cursor != 0 {
					if seen[cursor] {
						return nil, ErrRelationCycle
					}
					seen[cursor] = true
					chain = append(chain, cursor)
					nav.link("JournalEncounterSection", field, uint64(node.id), "section", uint64(cursor), depth+1)
					child, ok := nodes[cursor]
					if !ok {
						noteUnresolved(cursor)
						break
					}
					field = "NextSiblingSectionID"
					cursor = rowUint32(child.row, "NextSiblingSectionID")
				}
				for _, childID := range chain {
					child, ok := nodes[childID]
					if !ok {
						continue
					}
					if depth+1 > maxDepth {
						nav.setTruncation(TruncationMaxDepth)
						noteUnresolved(childID)
						continue
					}
					branch, err := build(child, depth+1)
					if err != nil {
						return nil, err
					}
					section.Children = append(section.Children, branch)
				}
				return section, nil
			}
			children := []*sectionNode{}
			for _, candidate := range sortedNodes(nodes) {
				if rowUint32(candidate.row, "ParentSectionID") == node.id {
					children = append(children, candidate)
				}
			}
			sortSections(children)
			for _, child := range children {
				if depth+1 > maxDepth {
					nav.setTruncation(TruncationMaxDepth)
					noteUnresolved(child.id)
					continue
				}
				nav.link("JournalEncounterSection", "ParentSectionID", uint64(child.id), "section", uint64(node.id), depth+1)
				branch, err := build(child, depth+1)
				if err != nil {
					return nil, err
				}
				section.Children = append(section.Children, branch)
			}
			return section, nil
		}
		tree := []*EncounterSection{}
		for _, root := range roots {
			branch, err := build(root, 0)
			if err != nil {
				return Reading[EncounterGetResult]{}, err
			}
			tree = append(tree, branch)
		}
		// Rows that belong to this encounter but never attach to a root are
		// reported, never silently dropped.
		reachable := map[uint32]bool{}
		var mark func([]*EncounterSection)
		mark = func(sections []*EncounterSection) {
			for _, section := range sections {
				reachable[section.ID] = true
				mark(section.Children)
			}
		}
		mark(tree)
		for _, node := range sortedNodes(nodes) {
			if !reachable[node.id] {
				noteUnresolved(node.id)
			}
		}
		spellIDs := []uint32{}
		var collect func([]*EncounterSection)
		collect = func(sections []*EncounterSection) {
			for _, section := range sections {
				if section.SpellID != 0 {
					spellIDs = addUnique(spellIDs, section.SpellID)
				}
				collect(section.Children)
			}
		}
		collect(tree)
		result := EncounterGetResult{
			JournalEncounterID: request.JournalEncounterID,
			SectionCount:       len(nodes), SpellCount: len(spellIDs), SpellIDs: spellIDs,
			Sections: tree, RelatedSpells: []RelatedSpell{}, RelatedFiles: []EncounterRelatedFile{},
			UnresolvedSectionIDs: unresolved,
		}
		if len(spellIDs) > 0 {
			spellNames, err := nav.open(ctx, "SpellName")
			if err != nil {
				return Reading[EncounterGetResult]{}, err
			}
			spellMisc, err := nav.open(ctx, "SpellMisc")
			if err != nil {
				return Reading[EncounterGetResult]{}, err
			}
			for _, spellID := range spellIDs {
				related := RelatedSpell{SpellID: spellID}
				if nameRow, ok, err := nav.rowByID(ctx, spellNames, spellID); err != nil {
					return Reading[EncounterGetResult]{}, err
				} else if ok {
					related.Name = nullableString(rowString(nameRow, "Name_lang"))
				}
				miscRows, _, err := nav.rowsByField(ctx, spellMisc, "SpellID", spellID, 1)
				if err != nil {
					return Reading[EncounterGetResult]{}, err
				}
				if len(miscRows) > 0 {
					icon := rowInt(miscRows[0].row, "SpellIconFileDataID")
					if icon > 0 {
						related.IconFileDataID = uint32(icon)
						nav.link("SpellMisc", "SpellIconFileDataID", uint64(miscRows[0].id), "icon_file", uint64(related.IconFileDataID), 1)
						result.RelatedFiles = append(result.RelatedFiles, EncounterRelatedFile{
							Kind: "icon_file", FileDataID: related.IconFileDataID,
							OwnerKind: "spell", OwnerID: spellID,
						})
					}
				}
				result.RelatedSpells = append(result.RelatedSpells, related)
			}
		}
		result.DataContext = nav.resultContext()
		manifest := RelationManifest{
			Operation: "encounter_get",
			Root:      "journal_encounter:" + uint32Text(request.JournalEncounterID),
			Snapshot:  result.Snapshot, DataBuild: result.Pin.FullBuild,
			Relations: result.Relations, RelatedFiles: result.RelatedFiles,
			Complete: result.Complete, Truncated: result.Truncated,
			TruncationReason: result.TruncationReason,
		}
		capture, err := nav.commitCapture(ctx, "encounter-get", "encounter:get:"+uint32Text(request.JournalEncounterID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[EncounterGetResult]{}, err
		}
		relationCapture, err := nav.commitCapture(ctx, "relation-manifest", "encounter:relations:"+uint32Text(request.JournalEncounterID), manifest, manifest.Complete, manifest.Truncated)
		if err != nil {
			return Reading[EncounterGetResult]{}, err
		}
		return Reading[EncounterGetResult]{Result: result, Captures: []evidence.CaptureRef{capture, relationCapture}}, nil
	})
}

func sortedNodes(nodes map[uint32]*sectionNode) []*sectionNode {
	out := make([]*sectionNode, 0, len(nodes))
	for _, node := range nodes {
		out = append(out, node)
	}
	slices.SortFunc(out, func(a, b *sectionNode) int {
		switch {
		case a.id < b.id:
			return -1
		case a.id > b.id:
			return 1
		}
		return 0
	})
	return out
}

// sortSections orders by (OrderIndex, ID), the legacy tree order.
func sortSections(sections []*sectionNode) {
	slices.SortFunc(sections, func(a, b *sectionNode) int {
		if a.order != b.order {
			if a.order < b.order {
				return -1
			}
			return 1
		}
		switch {
		case a.id < b.id:
			return -1
		case a.id > b.id:
			return 1
		}
		return 0
	})
}
