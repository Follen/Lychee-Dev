package records

import (
	"context"
	"slices"

	"github.com/follenfang/lycheedev/internal/evidence"
)

// Domain group: creature. Displays resolve by display ID or by model file
// data ID (both are first-class keys); models list every display of one model
// file with its texture and geoset variants.

// CreatureDisplayRequest addresses one display by display ID or by model file
// data ID; exactly one key must be set.
type CreatureDisplayRequest struct {
	DisplayID  uint32 `json:"displayID"`
	FileDataID uint32 `json:"fileDataID"`
}

type CreatureDisplayResult struct {
	DataContext
	DisplayID       uint32   `json:"displayID"`
	ModelID         uint32   `json:"modelID"`
	FileDataID      uint32   `json:"fileDataID"`
	ModelFileDataID uint32   `json:"modelFileDataID"`
	Textures        []uint32 `json:"textures"`
	Variations      []int    `json:"variations"`
}

// CreatureModelRequest selects every display rendered by one model file.
type CreatureModelRequest struct {
	FileDataID uint32 `json:"fileDataID"`
}

type CreatureModelDisplay struct {
	DisplayID  uint32   `json:"displayID"`
	ModelID    uint32   `json:"modelID"`
	FileDataID uint32   `json:"fileDataID"`
	Textures   []uint32 `json:"textures"`
	Variations []int    `json:"variations"`
}

type CreatureModelResult struct {
	DataContext
	FileDataID uint32                 `json:"fileDataID"`
	Displays   []CreatureModelDisplay `json:"displays"`
}

// creatureVariations expands CreatureDisplayInfoGeosetData rows to the legacy
// (GeosetIndex+1)*100+GeosetValue variant codes in ascending row order.
func (n *navigator) creatureVariations(ctx context.Context, t *preparedTable, displayID uint32) ([]int, error) {
	rows, extra, err := n.rowsByField(ctx, t, "CreatureDisplayInfoID", displayID, MaxRelatedRows)
	if err != nil {
		return nil, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	variations := []int{}
	for _, row := range rows {
		variations = append(variations, (rowInt(row.row, "GeosetIndex")+1)*100+rowInt(row.row, "GeosetValue"))
	}
	return variations, nil
}

// creatureModelFile resolves CreatureModelData.FileDataID for one model ID.
func (n *navigator) creatureModelFile(ctx context.Context, t *preparedTable, modelID uint32) (uint32, error) {
	if modelID == 0 {
		return 0, nil
	}
	row, ok, err := n.rowByID(ctx, t, modelID)
	if err != nil {
		return 0, err
	}
	if !ok {
		n.addPartial(PartialRelationUnresolved)
		return 0, nil
	}
	fileID := rowUint32(row, "FileDataID")
	if fileID != 0 {
		n.link("CreatureModelData", "FileDataID", uint64(modelID), "model_file", uint64(fileID), 1)
	}
	return fileID, nil
}

// creatureDisplayDetail assembles one display from its row.
func (n *navigator) creatureDisplayDetail(ctx context.Context, tables map[string]*preparedTable, displayID uint32, row map[string]any) (CreatureDisplayResult, error) {
	modelID := rowUint32(row, "ModelID")
	result := CreatureDisplayResult{
		DisplayID: displayID, ModelID: modelID,
		Textures: rowUint32List(row, "TextureVariationFileDataID"), Variations: []int{},
	}
	if modelID == 0 {
		n.addPartial(PartialRelationUnresolved)
	}
	fileID, err := n.creatureModelFile(ctx, tables["CreatureModelData"], modelID)
	if err != nil {
		return CreatureDisplayResult{}, err
	}
	result.FileDataID, result.ModelFileDataID = fileID, fileID
	variations, err := n.creatureVariations(ctx, tables["CreatureDisplayInfoGeosetData"], displayID)
	if err != nil {
		return CreatureDisplayResult{}, err
	}
	result.Variations = variations
	return result, nil
}

// InspectCreatureDisplay resolves one creature display by display ID or by
// model file data ID.
func InspectCreatureDisplay(ctx context.Context, root, snapshot string, query FileQuery, request CreatureDisplayRequest) (Reading[CreatureDisplayResult], error) {
	if (request.DisplayID == 0) == (request.FileDataID == 0) {
		return Reading[CreatureDisplayResult]{}, ErrRequestConflict
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[CreatureDisplayResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"CreatureDisplayInfo", "CreatureModelData", "CreatureDisplayInfoGeosetData"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[CreatureDisplayResult]{}, err
			}
			tables[name] = t
		}
		var result CreatureDisplayResult
		locator := ""
		if request.DisplayID != 0 {
			row, ok, err := nav.rowByID(ctx, tables["CreatureDisplayInfo"], request.DisplayID)
			if err != nil {
				return Reading[CreatureDisplayResult]{}, err
			}
			if !ok {
				return Reading[CreatureDisplayResult]{}, ErrRecordNotFound
			}
			result, err = nav.creatureDisplayDetail(ctx, tables, request.DisplayID, row)
			if err != nil {
				return Reading[CreatureDisplayResult]{}, err
			}
			locator = "creature:display:" + uint32Text(request.DisplayID)
		} else {
			displayID, row, err := nav.creatureDisplayByModelFile(ctx, tables, request.FileDataID)
			if err != nil {
				return Reading[CreatureDisplayResult]{}, err
			}
			if row == nil {
				return Reading[CreatureDisplayResult]{}, ErrRecordNotFound
			}
			result, err = nav.creatureDisplayDetail(ctx, tables, displayID, row)
			if err != nil {
				return Reading[CreatureDisplayResult]{}, err
			}
			locator = "creature:display:file:" + uint32Text(request.FileDataID)
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "creature-display", locator, result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[CreatureDisplayResult]{}, err
		}
		return Reading[CreatureDisplayResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// creatureDisplayByModelFile finds the lowest-row display of one model file.
func (n *navigator) creatureDisplayByModelFile(ctx context.Context, tables map[string]*preparedTable, fileDataID uint32) (uint32, map[string]any, error) {
	models, extra, err := n.rowsByField(ctx, tables["CreatureModelData"], "FileDataID", fileDataID, MaxRelatedRows)
	if err != nil {
		return 0, nil, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	if len(models) == 0 {
		return 0, nil, nil
	}
	if len(models) > 1 {
		n.addPartial(PartialRelationAmbiguous)
	}
	modelID := rowUint32(models[0].row, "ID")
	displays, extra, err := n.rowsByField(ctx, tables["CreatureDisplayInfo"], "ModelID", modelID, MaxRelatedRows)
	if err != nil {
		return 0, nil, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	if len(displays) == 0 {
		return 0, nil, nil
	}
	sorted := append([]rowWithID(nil), displays...)
	slices.SortFunc(sorted, func(a, b rowWithID) int {
		return int(rowUint32(a.row, "ID")) - int(rowUint32(b.row, "ID"))
	})
	for _, candidate := range sorted {
		n.link("CreatureDisplayInfo", "ModelID", uint64(candidate.id), "creature_display", uint64(rowUint32(candidate.row, "ID")), 0)
	}
	return rowUint32(sorted[0].row, "ID"), sorted[0].row, nil
}

// InspectCreatureModel lists every display of one model file with variants.
func InspectCreatureModel(ctx context.Context, root, snapshot string, query FileQuery, request CreatureModelRequest) (Reading[CreatureModelResult], error) {
	if request.FileDataID == 0 {
		return Reading[CreatureModelResult]{}, ErrRequestConflict
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[CreatureModelResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"CreatureDisplayInfo", "CreatureModelData", "CreatureDisplayInfoGeosetData"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[CreatureModelResult]{}, err
			}
			tables[name] = t
		}
		result := CreatureModelResult{FileDataID: request.FileDataID, Displays: []CreatureModelDisplay{}}
		models, extra, err := nav.rowsByField(ctx, tables["CreatureModelData"], "FileDataID", request.FileDataID, MaxRelatedRows)
		if err != nil {
			return Reading[CreatureModelResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		if len(models) > 1 {
			nav.addPartial(PartialRelationAmbiguous)
		}
		seen := map[uint32]bool{}
		for _, model := range models {
			modelID := rowUint32(model.row, "ID")
			displays, extra, err := nav.rowsByField(ctx, tables["CreatureDisplayInfo"], "ModelID", modelID, MaxRelatedRows)
			if err != nil {
				return Reading[CreatureModelResult]{}, err
			}
			if extra {
				nav.setTruncation(TruncationRowBudget)
			}
			sorted := append([]rowWithID(nil), displays...)
			slices.SortFunc(sorted, func(a, b rowWithID) int {
				return int(rowUint32(a.row, "ID")) - int(rowUint32(b.row, "ID"))
			})
			for _, display := range sorted {
				displayID := rowUint32(display.row, "ID")
				if seen[displayID] {
					continue
				}
				seen[displayID] = true
				variations, err := nav.creatureVariations(ctx, tables["CreatureDisplayInfoGeosetData"], displayID)
				if err != nil {
					return Reading[CreatureModelResult]{}, err
				}
				nav.link("CreatureDisplayInfo", "ModelID", uint64(display.id), "creature_display", uint64(displayID), 0)
				result.Displays = append(result.Displays, CreatureModelDisplay{
					DisplayID: displayID, ModelID: modelID, FileDataID: request.FileDataID,
					Textures: rowUint32List(display.row, "TextureVariationFileDataID"), Variations: variations,
				})
			}
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "creature-model", "creature:model:"+uint32Text(request.FileDataID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[CreatureModelResult]{}, err
		}
		return Reading[CreatureModelResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}
