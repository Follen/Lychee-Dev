package table

import (
	"context"
	"slices"
)

// Scan visits logical IDs (including copies) in ascending order without
// repeatedly scanning the identity map per page. The caller budgets the sorted
// identity index explicitly; only one decoded row is retained at a time.
func (v *View) Scan(ctx context.Context, indexBytes int64, textBytes int, yield func(map[string]any) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if yield == nil || textBytes <= 0 || textBytes > 1<<20 || indexBytes < 0 || int64(len(v.records.locations))*4 > indexBytes {
		return ErrLimit
	}
	ids := make([]uint32, 0, len(v.records.locations))
	for id := range v.records.locations {
		if err := ctx.Err(); err != nil {
			return err
		}
		ids = append(ids, id)
	}
	slices.Sort(ids)
	for _, id := range ids {
		row, err := v.Row(ctx, id, textBytes)
		if err != nil {
			return err
		}
		if err := yield(row); err != nil {
			return err
		}
	}
	return ctx.Err()
}
