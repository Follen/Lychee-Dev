package records

import (
	"context"

	"github.com/follenfang/lycheedev/internal/evidence"
)

// Domain group: decor. HouseDecor listings are explicitly bounded and keep
// every row (the legacy loader silently dropped rows without a model file).

// DecorListRequest bounds a HouseDecor listing.
type DecorListRequest struct {
	Limit int `json:"limit"`
}

// DecorItem is one HouseDecor row with its raw stored name text.
type DecorItem struct {
	ID                  uint32 `json:"id"`
	Name                string `json:"name"`
	ModelFileDataID     uint32 `json:"modelFileDataID"`
	ThumbnailFileDataID uint32 `json:"thumbnailFileDataID"`
	ItemID              uint32 `json:"itemID"`
	GameObjectID        uint32 `json:"gameObjectID"`
	Type                int    `json:"type"`
	ModelType           int    `json:"modelType"`
}

type DecorListResult struct {
	DataContext
	Count int         `json:"count"`
	Items []DecorItem `json:"items"`
}

// DecorGetRequest selects one decor by decor ID, item ID or model file data
// ID; exactly one key must be set.
type DecorGetRequest struct {
	ID              *uint32 `json:"id"`
	ItemID          *uint32 `json:"itemID"`
	ModelFileDataID *uint32 `json:"modelFileDataID"`
}

type DecorGetResult struct {
	DataContext
	Item DecorItem `json:"item"`
}

func decorItem(row map[string]any) DecorItem {
	return DecorItem{
		ID: rowUint32(row, "ID"), Name: rowString(row, "Name_lang"),
		ModelFileDataID:     rowUint32(row, "ModelFileDataID"),
		ThumbnailFileDataID: rowUint32(row, "ThumbnailFileDataID"),
		ItemID:              rowUint32(row, "ItemID"),
		GameObjectID:        rowUint32(row, "GameObjectID"),
		Type:                rowInt(row, "Type"), ModelType: rowInt(row, "ModelType"),
	}
}

// InspectDecorList lists HouseDecor rows in ascending ID order with an
// explicit bound and exact truncation reporting.
func InspectDecorList(ctx context.Context, root, snapshot string, query FileQuery, request DecorListRequest) (Reading[DecorListResult], error) {
	if err := validateLimit(request.Limit, MaxDecorListRows); err != nil {
		return Reading[DecorListResult]{}, err
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DecorListResult], error) {
		t, err := nav.open(ctx, "HouseDecor")
		if err != nil {
			return Reading[DecorListResult]{}, err
		}
		items := []DecorItem{}
		count, extra, err := nav.walkRows(ctx, t, request.Limit,
			func(map[string]any) (bool, error) { return true, nil },
			func(_ uint32, row map[string]any) error {
				items = append(items, decorItem(row))
				return nil
			})
		if err != nil {
			return Reading[DecorListResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationLimitReached)
		}
		result := DecorListResult{Count: count, Items: items}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "decor-list", "decor:list", result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[DecorListResult]{}, err
		}
		return Reading[DecorListResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectDecorGet resolves one decor by decor ID, item ID or model file data
// ID. Ambiguous keys resolve to the lowest row ID and every candidate stays in
// the recorded relations.
func InspectDecorGet(ctx context.Context, root, snapshot string, query FileQuery, request DecorGetRequest) (Reading[DecorGetResult], error) {
	keys := 0
	field, want := "", uint32(0)
	if request.ID != nil && *request.ID != 0 {
		keys, field, want = keys+1, "ID", *request.ID
	}
	if request.ItemID != nil && *request.ItemID != 0 {
		keys, field, want = keys+1, "ItemID", *request.ItemID
	}
	if request.ModelFileDataID != nil && *request.ModelFileDataID != 0 {
		keys, field, want = keys+1, "ModelFileDataID", *request.ModelFileDataID
	}
	if keys != 1 {
		return Reading[DecorGetResult]{}, ErrRequestConflict
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DecorGetResult], error) {
		t, err := nav.open(ctx, "HouseDecor")
		if err != nil {
			return Reading[DecorGetResult]{}, err
		}
		matches, extra, err := nav.rowsByField(ctx, t, field, want, MaxDecorListRows)
		if err != nil {
			return Reading[DecorGetResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationRowBudget)
		}
		if len(matches) == 0 {
			return Reading[DecorGetResult]{}, ErrRecordNotFound
		}
		if len(matches) > 1 {
			nav.addPartial(PartialRelationAmbiguous)
		}
		for _, match := range matches {
			nav.link("HouseDecor", field, uint64(match.id), "decor", uint64(rowUint32(match.row, "ID")), 0)
		}
		result := DecorGetResult{Item: decorItem(matches[0].row)}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "decor-get", "decor:get:"+field+":"+uint32Text(want), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[DecorGetResult]{}, err
		}
		return Reading[DecorGetResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}
