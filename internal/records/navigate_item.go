package records

import (
	"context"

	"github.com/follenfang/lycheedev/internal/evidence"
)

// Domain group: item. Appearance resolution walks
// ItemModifiedAppearance -> ItemAppearance -> ItemDisplayInfo and resolves
// model/texture file IDs through ModelFileData / TextureFileData. When several
// appearance rows exist the lowest row ID is selected and every candidate is
// kept as a relation (the legacy last-row-wins map silently dropped others).

// PartialVariantUnavailable reports a requested race/gender model variant that
// had to fall back to the first candidate.
const PartialVariantUnavailable = "variant_unavailable"

// ItemGetRequest selects one item summary.
type ItemGetRequest struct {
	ItemID uint32 `json:"itemID"`
}

// ItemSummary is the legacy item summary shape with raw stored name text.
type ItemSummary struct {
	ID            uint32 `json:"id"`
	Name          string `json:"name"`
	InventoryType int    `json:"inventoryType"`
	ClassID       int    `json:"classID"`
	SubclassID    int    `json:"subclassID"`
	Quality       int    `json:"quality"`
	SlotName      string `json:"slotName"`
}

type ItemGetResult struct {
	DataContext
	Item ItemSummary `json:"item"`
}

// ItemModelsRequest resolves model and texture file IDs for an item, with an
// optional race/gender model variant preference.
type ItemModelsRequest struct {
	ItemID uint32 `json:"itemID"`
	RaceID int    `json:"raceID"`
	Gender int    `json:"gender"`
}

type ItemModelsResult struct {
	DataContext
	ItemID      uint32   `json:"itemID"`
	RaceID      int      `json:"raceID"`
	Gender      int      `json:"gender"`
	DisplayID   uint32   `json:"displayID"`
	Models      []uint32 `json:"models"`
	Textures    []uint32 `json:"textures"`
	GeosetGroup []int    `json:"geosetGroup"`
}

// ItemGeosetRequest selects geoset and helmet-hide data for one item.
type ItemGeosetRequest struct {
	ItemID uint32 `json:"itemID"`
}

type ItemGeosetResult struct {
	DataContext
	ItemID          uint32 `json:"itemID"`
	DisplayID       uint32 `json:"displayID"`
	GeosetGroup     []int  `json:"geosetGroup"`
	HelmetGeosetVis []int  `json:"helmetGeosetVis"`
	HelmetHide      []int  `json:"helmetHide"`
}

// ItemTexturesRequest selects character texture sections for one item.
type ItemTexturesRequest struct {
	ItemID uint32 `json:"itemID"`
}

type ItemTextureSection struct {
	Section    int    `json:"section"`
	FileDataID uint32 `json:"fileDataID"`
}

type ItemTexturesResult struct {
	DataContext
	ItemID    uint32               `json:"itemID"`
	DisplayID uint32               `json:"displayID"`
	Textures  []ItemTextureSection `json:"textures"`
}

// inventory slot names indexed by inventory type (the legacy name set with
// the scrambled 6..10 indices corrected; see the report).
var itemSlotNames = map[int]string{
	0: "None", 1: "Head", 2: "Neck", 3: "Shoulder", 4: "Shirt",
	5: "Chest", 6: "Waist", 7: "Legs", 8: "Feet", 9: "Wrist",
	10: "Hands", 11: "Finger", 12: "Trinket", 13: "One-Hand",
	14: "Shield", 15: "Ranged", 16: "Back", 17: "Two-Hand",
	18: "Bag", 19: "Tabard", 20: "Robe", 21: "Main Hand",
	22: "Off Hand", 23: "Held In Off-Hand", 24: "Ammo", 25: "Thrown",
	26: "Ranged Right", 27: "Quiver", 28: "Relic",
}

func itemSlotName(inventoryType int) string {
	if name, ok := itemSlotNames[inventoryType]; ok {
		return name
	}
	return "Unknown"
}

// itemAppearance resolves the item's display row through the appearance
// chain. The returned row is the ItemDisplayInfo row.
func (n *navigator) itemAppearance(ctx context.Context, tables map[string]*preparedTable, itemID uint32) (displayID uint32, row map[string]any, err error) {
	appearances, extra, err := n.rowsByField(ctx, tables["ItemModifiedAppearance"], "ItemID", itemID, MaxRelatedRows)
	if err != nil {
		return 0, nil, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	if len(appearances) == 0 {
		return 0, nil, nil
	}
	if len(appearances) > 1 {
		n.addPartial(PartialRelationAmbiguous)
	}
	for _, candidate := range appearances[1:] {
		n.link("ItemModifiedAppearance", "ItemAppearanceID", uint64(candidate.id), "item_appearance", uint64(rowUint32(candidate.row, "ItemAppearanceID")), 0)
	}
	selected := appearances[0]
	appearanceID := rowUint32(selected.row, "ItemAppearanceID")
	n.link("ItemModifiedAppearance", "ItemAppearanceID", uint64(selected.id), "item_appearance", uint64(appearanceID), 0)
	appearance, ok, err := n.rowByID(ctx, tables["ItemAppearance"], appearanceID)
	if err != nil {
		return 0, nil, err
	}
	if !ok {
		n.addPartial(PartialRelationUnresolved)
		return 0, nil, nil
	}
	displayID = rowUint32(appearance, "ItemDisplayInfoID")
	n.link("ItemAppearance", "ItemDisplayInfoID", uint64(appearanceID), "item_display", uint64(displayID), 1)
	display, ok, err := n.rowByID(ctx, tables["ItemDisplayInfo"], displayID)
	if err != nil {
		return 0, nil, err
	}
	if !ok {
		n.addPartial(PartialRelationUnresolved)
		return 0, nil, nil
	}
	return displayID, display, nil
}

// itemModelFile resolves one ModelResourcesID to a model file ID, applying the
// requested race/gender variant when present.
func (n *navigator) itemModelFile(ctx context.Context, tables map[string]*preparedTable, displayID, resourceID uint32, raceID, gender int, depth int) (uint32, error) {
	candidates, extra, err := n.rowsByField(ctx, tables["ModelFileData"], "ModelResourcesID", resourceID, MaxRelatedRows)
	if err != nil {
		return 0, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	n.link("ItemDisplayInfo", "ModelResourcesID", uint64(displayID), "model_resource", uint64(resourceID), depth)
	files := []rowWithID{}
	for _, candidate := range candidates {
		fileID := rowUint32(candidate.row, "FileDataID")
		if fileID == 0 {
			fileID = rowUint32(candidate.row, "ID")
		}
		if fileID != 0 {
			n.link("ModelFileData", "FileDataID", uint64(candidate.id), "model_file", uint64(fileID), depth+1)
			files = append(files, rowWithID{id: fileID, row: candidate.row})
		}
	}
	if len(files) == 0 {
		return 0, nil
	}
	if raceID == 0 && gender == 0 {
		return files[0].id, nil
	}
	for _, file := range files {
		component, ok, err := n.rowByID(ctx, tables["ComponentModelFileData"], file.id)
		if err != nil {
			return 0, err
		}
		if ok && rowInt(component, "RaceID") == raceID && rowInt(component, "GenderIndex") == gender {
			return file.id, nil
		}
	}
	n.addPartial(PartialVariantUnavailable)
	return files[0].id, nil
}

// itemTextureFile resolves one MaterialResourcesID to its texture file ID.
// Only rows with UsageType 0 qualify, in ascending row order (first wins).
func (n *navigator) itemTextureFile(ctx context.Context, tables map[string]*preparedTable, materialID uint32, depth int) (uint32, error) {
	candidates, extra, err := n.rowsByField(ctx, tables["TextureFileData"], "MaterialResourcesID", materialID, MaxRelatedRows)
	if err != nil {
		return 0, err
	}
	if extra {
		n.setTruncation(TruncationRowBudget)
	}
	for _, candidate := range candidates {
		if rowUint32(candidate.row, "UsageType") != 0 {
			continue
		}
		fileID := rowUint32(candidate.row, "FileDataID")
		if fileID == 0 {
			fileID = rowUint32(candidate.row, "ID")
		}
		if fileID == 0 {
			continue
		}
		n.link("TextureFileData", "MaterialResourcesID", uint64(candidate.id), "texture_file", uint64(fileID), depth)
		return fileID, nil
	}
	return 0, nil
}

// requireItem checks the item exists in ItemSparse (the legacy identity row).
func (n *navigator) requireItem(ctx context.Context, t *preparedTable, itemID uint32) (map[string]any, error) {
	row, ok, err := n.rowByID(ctx, t, itemID)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, ErrRecordNotFound
	}
	return row, nil
}

// InspectItem returns the item summary and slot name.
func InspectItem(ctx context.Context, root, snapshot string, query FileQuery, request ItemGetRequest) (Reading[ItemGetResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[ItemGetResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"Item", "ItemSparse"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[ItemGetResult]{}, err
			}
			tables[name] = t
		}
		sparse, err := nav.requireItem(ctx, tables["ItemSparse"], request.ItemID)
		if err != nil {
			return Reading[ItemGetResult]{}, err
		}
		summary := ItemSummary{
			ID: request.ItemID, Name: rowString(sparse, "Display_lang"),
			InventoryType: rowInt(sparse, "InventoryType"),
			Quality:       rowInt(sparse, "OverallQualityID"),
		}
		summary.SlotName = itemSlotName(summary.InventoryType)
		if class, ok, err := nav.rowByID(ctx, tables["Item"], request.ItemID); err != nil {
			return Reading[ItemGetResult]{}, err
		} else if ok {
			summary.ClassID = rowInt(class, "ClassID")
			summary.SubclassID = rowInt(class, "SubclassID")
		} else {
			nav.addPartial(PartialRowMissing)
		}
		result := ItemGetResult{Item: summary}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "item-get", "item:get:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[ItemGetResult]{}, err
		}
		return Reading[ItemGetResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectItemModels resolves model and texture file IDs for one item.
func InspectItemModels(ctx context.Context, root, snapshot string, query FileQuery, request ItemModelsRequest) (Reading[ItemModelsResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[ItemModelsResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"ItemSparse", "ItemModifiedAppearance", "ItemAppearance", "ItemDisplayInfo", "ModelFileData", "TextureFileData", "ComponentModelFileData"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[ItemModelsResult]{}, err
			}
			tables[name] = t
		}
		if _, err := nav.requireItem(ctx, tables["ItemSparse"], request.ItemID); err != nil {
			return Reading[ItemModelsResult]{}, err
		}
		result := ItemModelsResult{
			ItemID: request.ItemID, RaceID: request.RaceID, Gender: request.Gender,
			Models: []uint32{}, Textures: []uint32{}, GeosetGroup: []int{},
		}
		displayID, display, err := nav.itemAppearance(ctx, tables, request.ItemID)
		if err != nil {
			return Reading[ItemModelsResult]{}, err
		}
		result.DisplayID = displayID
		if display == nil {
			result.DataContext = nav.resultContext()
			capture, err := nav.commitCapture(ctx, "item-models", "item:models:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
			if err != nil {
				return Reading[ItemModelsResult]{}, err
			}
			return Reading[ItemModelsResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
		}
		for _, resourceID := range rowUint32List(display, "ModelResourcesID") {
			fileID, err := nav.itemModelFile(ctx, tables, displayID, resourceID, request.RaceID, request.Gender, 2)
			if err != nil {
				return Reading[ItemModelsResult]{}, err
			}
			if fileID != 0 {
				result.Models = append(result.Models, fileID)
			}
		}
		for _, materialID := range rowUint32List(display, "ModelMaterialResourcesID") {
			fileID, err := nav.itemTextureFile(ctx, tables, materialID, 2)
			if err != nil {
				return Reading[ItemModelsResult]{}, err
			}
			if fileID != 0 {
				result.Textures = append(result.Textures, fileID)
			}
		}
		result.GeosetGroup = rowIntList(display, "GeosetGroup")
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "item-models", "item:models:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[ItemModelsResult]{}, err
		}
		return Reading[ItemModelsResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectItemGeosets returns geoset and helmet-hide data for one item.
func InspectItemGeosets(ctx context.Context, root, snapshot string, query FileQuery, request ItemGeosetRequest) (Reading[ItemGeosetResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[ItemGeosetResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"ItemSparse", "ItemModifiedAppearance", "ItemAppearance", "ItemDisplayInfo", "HelmetGeosetData"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[ItemGeosetResult]{}, err
			}
			tables[name] = t
		}
		if _, err := nav.requireItem(ctx, tables["ItemSparse"], request.ItemID); err != nil {
			return Reading[ItemGeosetResult]{}, err
		}
		result := ItemGeosetResult{
			ItemID: request.ItemID, GeosetGroup: []int{},
			HelmetGeosetVis: []int{}, HelmetHide: []int{},
		}
		displayID, display, err := nav.itemAppearance(ctx, tables, request.ItemID)
		if err != nil {
			return Reading[ItemGeosetResult]{}, err
		}
		result.DisplayID = displayID
		if display != nil {
			result.GeosetGroup = rowIntList(display, "GeosetGroup")
			result.HelmetGeosetVis = rowIntList(display, "HelmetGeosetVis")
		}
		for _, vis := range result.HelmetGeosetVis {
			if vis <= 0 {
				continue
			}
			rows, extra, err := nav.rowsByField(ctx, tables["HelmetGeosetData"], "HelmetGeosetVisDataID", uint32(vis), MaxRelatedRows)
			if err != nil {
				return Reading[ItemGeosetResult]{}, err
			}
			if extra {
				nav.setTruncation(TruncationRowBudget)
			}
			for _, row := range rows {
				nav.link("HelmetGeosetData", "HelmetGeosetVisDataID", uint64(row.id), "helmet_geoset_vis", uint64(vis), 2)
				result.HelmetHide = append(result.HelmetHide, rowInt(row.row, "HideGeosetGroup"))
			}
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "item-geosets", "item:geosets:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[ItemGeosetResult]{}, err
		}
		return Reading[ItemGeosetResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectItemTextures returns character texture sections for one item.
func InspectItemTextures(ctx context.Context, root, snapshot string, query FileQuery, request ItemTexturesRequest) (Reading[ItemTexturesResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[ItemTexturesResult], error) {
		tables := map[string]*preparedTable{}
		for _, name := range []string{"ItemSparse", "ItemModifiedAppearance", "ItemAppearance", "ItemDisplayInfo", "ItemDisplayInfoMaterialRes", "TextureFileData"} {
			t, err := nav.open(ctx, name)
			if err != nil {
				return Reading[ItemTexturesResult]{}, err
			}
			tables[name] = t
		}
		if _, err := nav.requireItem(ctx, tables["ItemSparse"], request.ItemID); err != nil {
			return Reading[ItemTexturesResult]{}, err
		}
		result := ItemTexturesResult{ItemID: request.ItemID, Textures: []ItemTextureSection{}}
		displayID, _, err := nav.itemAppearance(ctx, tables, request.ItemID)
		if err != nil {
			return Reading[ItemTexturesResult]{}, err
		}
		result.DisplayID = displayID
		if displayID == 0 {
			result.DataContext = nav.resultContext()
			capture, err := nav.commitCapture(ctx, "item-textures", "item:textures:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
			if err != nil {
				return Reading[ItemTexturesResult]{}, err
			}
			return Reading[ItemTexturesResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
		}
		sections, extra, err := nav.rowsByField(ctx, tables["ItemDisplayInfoMaterialRes"], "ItemDisplayInfoID", displayID, MaxRelatedRows)
		if err != nil {
			return Reading[ItemTexturesResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		for _, section := range sections {
			materialID := rowUint32(section.row, "MaterialResourcesID")
			if materialID == 0 {
				continue
			}
			fileID, err := nav.itemTextureFile(ctx, tables, materialID, 2)
			if err != nil {
				return Reading[ItemTexturesResult]{}, err
			}
			if fileID == 0 {
				continue
			}
			result.Textures = append(result.Textures, ItemTextureSection{Section: rowInt(section.row, "ComponentSection"), FileDataID: fileID})
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "item-textures", "item:textures:"+uint32Text(request.ItemID), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[ItemTexturesResult]{}, err
		}
		return Reading[ItemTexturesResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}
