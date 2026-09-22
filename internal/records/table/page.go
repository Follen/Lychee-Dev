package table

import (
	"container/heap"
	"context"
	"encoding/json"
	"sort"
)

type RowPage struct {
	After *uint32          `json:"after"`
	Next  *uint32          `json:"next"`
	More  bool             `json:"more"`
	IDs   []uint32         `json:"ids"`
	Rows  []map[string]any `json:"rows"`
}

// Page selects IDs in ascending logical order, including copies. A nil cursor
// includes ID zero; a cursor excludes that ID. Selection uses O(limit) additional
// storage, not another sorted copy of the entire identity index. Callers must
// carry the same snapshot/table identity along with the numeric cursor.
func (v *View) Page(ctx context.Context, after *uint32, limit, byteBudget int) (RowPage, error) {
	if err := ctx.Err(); err != nil {
		return RowPage{}, err
	}
	if limit < 1 || limit > 200 || byteBudget < 2 || byteBudget > 16<<20 {
		return RowPage{}, ErrLimit
	}
	var cursor *uint32
	if after != nil {
		value := *after
		cursor = &value
	}
	selected := make(idHeap, 0, limit+1)
	for id := range v.records.locations {
		if err := ctx.Err(); err != nil {
			return RowPage{}, err
		}
		if cursor != nil && id <= *cursor {
			continue
		}
		if len(selected) < limit+1 {
			heap.Push(&selected, id)
		} else if id < selected[0] {
			selected[0] = id
			heap.Fix(&selected, 0)
		}
	}
	sort.Slice(selected, func(i, j int) bool { return selected[i] < selected[j] })
	page := RowPage{After: cursor, More: len(selected) > limit, IDs: []uint32{}, Rows: []map[string]any{}}
	if page.More {
		selected = selected[:limit]
	}
	remaining := byteBudget - 2 // Brackets and commas count against the row-array budget.
	for _, id := range selected {
		row, err := v.Row(ctx, id, 1<<20)
		if err != nil {
			return RowPage{}, err
		}
		raw, err := json.Marshal(row)
		if err != nil {
			return RowPage{}, err
		}
		cost := len(raw)
		if len(page.Rows) > 0 {
			cost++
		}
		if cost > remaining {
			return RowPage{}, ErrLimit
		}
		remaining -= cost
		page.IDs = append(page.IDs, id)
		page.Rows = append(page.Rows, row)
	}
	if page.More {
		next := page.IDs[len(page.IDs)-1]
		page.Next = &next
	}
	return page, nil
}

// Max-heap retains only the smallest limit+1 candidates while scanning the map.
type idHeap []uint32

func (h idHeap) Len() int           { return len(h) }
func (h idHeap) Less(i, j int) bool { return h[i] > h[j] }
func (h idHeap) Swap(i, j int)      { h[i], h[j] = h[j], h[i] }
func (h *idHeap) Push(value any)    { *h = append(*h, value.(uint32)) }
func (h *idHeap) Pop() any {
	values := *h
	last := values[len(values)-1]
	*h = values[:len(values)-1]
	return last
}
