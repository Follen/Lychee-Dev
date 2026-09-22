package records

import (
	"context"
	"regexp"
	"slices"

	"github.com/follenfang/lycheedev/internal/evidence"
)

// Domain group: spell. The info query traverses trigger chains and
// description references with a depth bound (default 5) and cycle handling;
// auras and summons read the SpellEffect table with legacy presence and
// summon-effect semantics.

// SpellInfoRequest seeds one trigger/description traversal.
type SpellInfoRequest struct {
	SpellID  uint32 `json:"spellID"`
	MaxDepth int    `json:"maxDepth"`
}

type SpellCastTimeDetail struct {
	Base    int `json:"base"`
	Minimum int `json:"minimum"`
}

type SpellDurationDetail struct {
	Duration    int `json:"duration"`
	MaxDuration int `json:"maxDuration"`
}

type SpellRangeDetail struct {
	DisplayName string `json:"displayName"`
	RangeMin    []int  `json:"rangeMin"`
	RangeMax    []int  `json:"rangeMax"`
}

type SpellMiscDetail struct {
	Attributes          []int                `json:"attributes"`
	SchoolMask          int                  `json:"schoolMask"`
	Speed               int                  `json:"speed"`
	SpellIconFileDataID int                  `json:"spellIconFileDataID"`
	CastTime            *SpellCastTimeDetail `json:"castTime"`
	Duration            *SpellDurationDetail `json:"duration"`
	Range               *SpellRangeDetail    `json:"range"`
}

type SpellEffectDetail struct {
	EffectIndex        int     `json:"effectIndex"`
	Effect             int     `json:"effect"`
	EffectAura         int     `json:"effectAura"`
	EffectTriggerSpell *uint32 `json:"effectTriggerSpell"`
	EffectAuraPeriod   *uint32 `json:"effectAuraPeriod"`
	EffectBasePoints   int     `json:"effectBasePoints"`
	EffectMechanic     int     `json:"effectMechanic"`
	ImplicitTarget     []int   `json:"implicitTarget"`
	EffectRadiusIndex  []int   `json:"effectRadiusIndex"`
	EffectMiscValue    []int   `json:"effectMiscValue"`
}

type SpellDetail struct {
	SpellID         uint32              `json:"spellID"`
	Name            *string             `json:"name"`
	Description     *string             `json:"description"`
	AuraDescription *string             `json:"auraDescription"`
	IsSeed          bool                `json:"isSeed"`
	TriggeredBy     []uint32            `json:"triggeredBy"`
	Misc            *SpellMiscDetail    `json:"misc"`
	Effects         []SpellEffectDetail `json:"effects"`
}

// SpellInfoResult is the bounded trigger-chain and description-reference
// closure around one seed spell.
type SpellInfoResult struct {
	DataContext
	SpellID    uint32                 `json:"spellID"`
	SeedCount  int                    `json:"seedCount"`
	TotalCount int                    `json:"totalCount"`
	ChainDepth int                    `json:"chainDepth"`
	Triggers   map[uint32][]uint32    `json:"triggers"`
	DescRefs   map[uint32][]uint32    `json:"descRefs"`
	Spells     map[uint32]SpellDetail `json:"spells"`
}

// SpellAurasRequest checks aura presence for one spell.
type SpellAurasRequest struct {
	SpellID uint32 `json:"spellID"`
}

// SpellAurasResult preserves the legacy presence/absence list semantics: the
// spell appears in exactly one of hasAura or noAura.
type SpellAurasResult struct {
	DataContext
	SpellID uint32   `json:"spellID"`
	HasAura []uint32 `json:"hasAura"`
	NoAura  []uint32 `json:"noAura"`
}

// SpellSummonsRequest lists NPC summon effects of one spell, optionally
// filtered to one NPC.
type SpellSummonsRequest struct {
	SpellID uint32 `json:"spellID"`
	NPCID   uint32 `json:"npcID"`
}

// SpellSummon is one summon effect with its exact decoded effect row.
type SpellSummon struct {
	SpellID     uint32         `json:"spellID"`
	NPCID       uint32         `json:"npcID"`
	EffectIndex int            `json:"effectIndex"`
	Row         map[string]any `json:"row"`
}

type SpellSummonsResult struct {
	DataContext
	SpellID uint32        `json:"spellID"`
	NPCID   uint32        `json:"npcID"`
	Count   int           `json:"count"`
	Summons []SpellSummon `json:"summons"`
}

// spellReferencePattern matches "$@spellname123" and "$12345x" style
// description references. Short numeric references (four or fewer digits) are
// not spell references in this data model; that boundary is preserved from
// the legacy oracle semantics.
var spellReferencePattern = regexp.MustCompile(`\$@spellname(\d+)|\$(\d{5,})[a-zA-Z]`)

func spellReferences(text string) []uint32 {
	out := []uint32{}
	for _, match := range spellReferencePattern.FindAllStringSubmatch(text, -1) {
		value := parseReferenceID(match[1])
		if value == 0 {
			value = parseReferenceID(match[2])
		}
		if value != 0 {
			out = append(out, value)
		}
	}
	return out
}

func parseReferenceID(value string) uint32 {
	if value == "" {
		return 0
	}
	var parsed uint64
	for _, c := range value {
		if c < '0' || c > '9' {
			return 0
		}
		parsed = parsed*10 + uint64(c-'0')
		if parsed > 0xffffffff {
			return 0
		}
	}
	return uint32(parsed)
}

func addUnique(values []uint32, value uint32) []uint32 {
	if slices.Contains(values, value) {
		return values
	}
	return append(values, value)
}

// spellEffectChildren lists the spells one effect triggers: its trigger spell
// and, for effect 64, its misc value (legacy semantics).
func spellEffectChildren(row map[string]any) (trigger uint32, misc uint32) {
	trigger = rowUint32(row, "EffectTriggerSpell")
	if rowInt(row, "Effect") == 64 {
		misc = rowFirstUint32(row, "EffectMiscValue")
	}
	return trigger, misc
}

// InspectSpellInfo resolves the trigger and description closure around one
// spell with full per-spell detail for every collected spell.
func InspectSpellInfo(ctx context.Context, root, snapshot string, query FileQuery, request SpellInfoRequest) (Reading[SpellInfoResult], error) {
	maxDepth := request.MaxDepth
	if maxDepth <= 0 {
		maxDepth = DefaultSpellDepth
	}
	if maxDepth > MaxSpellDepth {
		return Reading[SpellInfoResult]{}, ErrQueryLimit
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[SpellInfoResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"SpellName", "Spell", "SpellEffect", "SpellMisc", "SpellCastTimes", "SpellDuration", "SpellRange"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[SpellInfoResult]{}, err
			}
			tables[name] = t
		}
		seed := request.SpellID
		allIDs := map[uint32]bool{seed: true}
		triggers := map[uint32][]uint32{}
		descRefs := map[uint32][]uint32{}
		effectRows := map[uint32][]rowWithID{}
		frontier := []uint32{seed}
		depth := 0
		for len(frontier) > 0 && depth < maxDepth {
			next := []uint32{}
			for _, sid := range frontier {
				effects, extra, err := nav.rowsByField(ctx, tables["SpellEffect"], "SpellID", sid, MaxRelatedRows)
				if err != nil {
					return Reading[SpellInfoResult]{}, err
				}
				if extra {
					nav.setTruncation(TruncationRowBudget)
				}
				effectRows[sid] = effects
				for _, effect := range effects {
					trigger, misc := spellEffectChildren(effect.row)
					expand := func(child uint32, field string) {
						if child == 0 || child == sid {
							return
						}
						nav.link("SpellEffect", field, uint64(effect.id), "spell", uint64(child), depth)
						triggers[sid] = addUnique(triggers[sid], child)
						if !allIDs[child] {
							allIDs[child] = true
							next = append(next, child)
						}
					}
					expand(trigger, "EffectTriggerSpell")
					if misc != trigger {
						expand(misc, "EffectMiscValue")
					}
				}
				spellRow, ok, err := nav.rowByID(ctx, tables["Spell"], sid)
				if err != nil {
					return Reading[SpellInfoResult]{}, err
				}
				if ok {
					for _, ref := range spellReferences(rowString(spellRow, "Description_lang")) {
						if ref == sid {
							continue
						}
						nav.link("Spell", "Description_lang", uint64(sid), "spell", uint64(ref), depth)
						descRefs[sid] = addUnique(descRefs[sid], ref)
						if !allIDs[ref] {
							allIDs[ref] = true
							next = append(next, ref)
						}
					}
					for _, ref := range spellReferences(rowString(spellRow, "AuraDescription_lang")) {
						if ref == sid {
							continue
						}
						nav.link("Spell", "AuraDescription_lang", uint64(sid), "spell", uint64(ref), depth)
						descRefs[sid] = addUnique(descRefs[sid], ref)
						if !allIDs[ref] {
							allIDs[ref] = true
							next = append(next, ref)
						}
					}
				}
			}
			frontier = next
			depth++
		}
		if len(frontier) > 0 {
			// The depth bound stopped with an open frontier: honest truncation.
			nav.setTruncation(TruncationMaxDepth)
		}
		triggeredBy := map[uint32][]uint32{}
		for parent, children := range triggers {
			for _, child := range children {
				triggeredBy[child] = addUnique(triggeredBy[child], parent)
			}
		}
		for parent, children := range descRefs {
			for _, child := range children {
				triggeredBy[child] = addUnique(triggeredBy[child], parent)
			}
		}
		spells := map[uint32]SpellDetail{}
		for _, sid := range sortedIDs(allIDs) {
			effects := effectRows[sid]
			if effects == nil {
				loaded, extra, err := nav.rowsByField(ctx, tables["SpellEffect"], "SpellID", sid, MaxRelatedRows)
				if err != nil {
					return Reading[SpellInfoResult]{}, err
				}
				if extra {
					nav.setTruncation(TruncationRowBudget)
				}
				effects, effectRows[sid] = loaded, loaded
			}
			payloads := []SpellEffectDetail{}
			for _, effect := range effects {
				payloads = append(payloads, spellEffectPayload(effect.row))
			}
			nameRow, _, err := nav.rowByID(ctx, tables["SpellName"], sid)
			if err != nil {
				return Reading[SpellInfoResult]{}, err
			}
			var name *string
			if nameRow != nil {
				name = nullableString(rowString(nameRow, "Name_lang"))
			}
			spellRow, _, err := nav.rowByID(ctx, tables["Spell"], sid)
			if err != nil {
				return Reading[SpellInfoResult]{}, err
			}
			var description, aura *string
			if spellRow != nil {
				description = nullableString(rowString(spellRow, "Description_lang"))
				aura = nullableString(rowString(spellRow, "AuraDescription_lang"))
			}
			misc, err := nav.spellMisc(ctx, tables, sid)
			if err != nil {
				return Reading[SpellInfoResult]{}, err
			}
			parents := triggeredBy[sid]
			if parents == nil {
				parents = []uint32{}
			}
			slices.Sort(parents)
			spells[sid] = SpellDetail{
				SpellID: sid, Name: name, Description: description, AuraDescription: aura,
				IsSeed: sid == seed, TriggeredBy: parents, Misc: misc, Effects: payloads,
			}
		}
		result := SpellInfoResult{
			SpellID: seed, SeedCount: 1, TotalCount: len(allIDs), ChainDepth: depth,
			Triggers: triggers, DescRefs: descRefs, Spells: spells,
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "spell-info", spellLocator("info", seed), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[SpellInfoResult]{}, err
		}
		return Reading[SpellInfoResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// spellMisc resolves SpellMisc plus its cast/duration/range rows for one spell.
func (n *navigator) spellMisc(ctx context.Context, tables map[string]*preparedTable, sid uint32) (*SpellMiscDetail, error) {
	rows, _, err := n.rowsByField(ctx, tables["SpellMisc"], "SpellID", sid, 1)
	if err != nil || len(rows) == 0 {
		return nil, err
	}
	row := rows[0].row
	detail := &SpellMiscDetail{
		Attributes: rowIntList(row, "Attributes"), SchoolMask: rowInt(row, "SchoolMask"),
		Speed: rowInt(row, "Speed"), SpellIconFileDataID: rowInt(row, "SpellIconFileDataID"),
	}
	if detail.SpellIconFileDataID > 0 {
		n.link("SpellMisc", "SpellIconFileDataID", uint64(rows[0].id), "icon_file", uint64(detail.SpellIconFileDataID), 1)
	}
	if cast, ok, err := n.rowByID(ctx, tables["SpellCastTimes"], rowUint32(row, "CastingTimeIndex")); err != nil {
		return nil, err
	} else if ok {
		detail.CastTime = &SpellCastTimeDetail{Base: rowInt(cast, "Base"), Minimum: rowInt(cast, "Minimum")}
	}
	if duration, ok, err := n.rowByID(ctx, tables["SpellDuration"], rowUint32(row, "DurationIndex")); err != nil {
		return nil, err
	} else if ok {
		detail.Duration = &SpellDurationDetail{Duration: rowInt(duration, "Duration"), MaxDuration: rowInt(duration, "MaxDuration")}
	}
	if ranged, ok, err := n.rowByID(ctx, tables["SpellRange"], rowUint32(row, "RangeIndex")); err != nil {
		return nil, err
	} else if ok {
		detail.Range = &SpellRangeDetail{
			DisplayName: rowString(ranged, "DisplayName_lang"),
			RangeMin:    rowIntList(ranged, "RangeMin"), RangeMax: rowIntList(ranged, "RangeMax"),
		}
	}
	return detail, nil
}

func spellEffectPayload(row map[string]any) SpellEffectDetail {
	return SpellEffectDetail{
		EffectIndex:        rowInt(row, "EffectIndex"),
		Effect:             rowInt(row, "Effect"),
		EffectAura:         rowInt(row, "EffectAura"),
		EffectTriggerSpell: nullableUint32(rowUint32(row, "EffectTriggerSpell")),
		EffectAuraPeriod:   nullableUint32(rowUint32(row, "EffectAuraPeriod")),
		EffectBasePoints:   rowInt(row, "EffectBasePointsF"),
		EffectMechanic:     rowInt(row, "EffectMechanic"),
		ImplicitTarget:     rowIntList(row, "ImplicitTarget"),
		EffectRadiusIndex:  rowIntList(row, "EffectRadiusIndex"),
		EffectMiscValue:    rowIntList(row, "EffectMiscValue"),
	}
}

func spellLocator(verb string, spellID uint32) string {
	return "spell:" + verb + ":" + uint32Text(spellID)
}

// InspectSpellAuras reports aura presence/absence from SpellEffect rows.
func InspectSpellAuras(ctx context.Context, root, snapshot string, query FileQuery, request SpellAurasRequest) (Reading[SpellAurasResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[SpellAurasResult], error) {
		t, err := nav.open(ctx, "SpellEffect")
		if err != nil {
			return Reading[SpellAurasResult]{}, err
		}
		effects, extra, err := nav.rowsByField(ctx, t, "SpellID", request.SpellID, MaxRelatedRows)
		if err != nil {
			return Reading[SpellAurasResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		has := false
		for _, effect := range effects {
			if rowInt(effect.row, "Effect") == 6 {
				has = true
			}
			nav.link("SpellEffect", "SpellID", uint64(effect.id), "spell", uint64(request.SpellID), 0)
		}
		result := SpellAurasResult{SpellID: request.SpellID, HasAura: []uint32{}, NoAura: []uint32{}}
		if has {
			result.HasAura = []uint32{request.SpellID}
		} else {
			result.NoAura = []uint32{request.SpellID}
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "spell-auras", spellLocator("auras", request.SpellID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[SpellAurasResult]{}, err
		}
		return Reading[SpellAurasResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectSpellSummons lists summon effects (effect 28) with their NPC from
// EffectMiscValue, optionally filtered to one NPC.
func InspectSpellSummons(ctx context.Context, root, snapshot string, query FileQuery, request SpellSummonsRequest) (Reading[SpellSummonsResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[SpellSummonsResult], error) {
		t, err := nav.open(ctx, "SpellEffect")
		if err != nil {
			return Reading[SpellSummonsResult]{}, err
		}
		effects, extra, err := nav.rowsByField(ctx, t, "SpellID", request.SpellID, MaxRelatedRows)
		if err != nil {
			return Reading[SpellSummonsResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		summons := []SpellSummon{}
		for _, effect := range effects {
			if rowInt(effect.row, "Effect") != 28 {
				continue
			}
			entry := SpellSummon{
				SpellID: request.SpellID, NPCID: rowFirstUint32(effect.row, "EffectMiscValue"),
				EffectIndex: rowInt(effect.row, "EffectIndex"), Row: effect.row,
			}
			if request.NPCID != 0 && entry.NPCID != request.NPCID {
				continue
			}
			if entry.NPCID != 0 {
				nav.link("SpellEffect", "EffectMiscValue", uint64(effect.id), "npc", uint64(entry.NPCID), 0)
			}
			summons = append(summons, entry)
		}
		result := SpellSummonsResult{
			SpellID: request.SpellID, NPCID: request.NPCID,
			Count: len(summons), Summons: summons,
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "spell-summons", spellLocator("summons", request.SpellID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[SpellSummonsResult]{}, err
		}
		return Reading[SpellSummonsResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}
