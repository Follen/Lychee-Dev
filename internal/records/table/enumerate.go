package table

import (
	"context"
	"slices"
)

// LogicalCount returns the number of logical record IDs, including copy-table
// aliases. It is an index property, not a row decode and not a table size claim.
func (v *View) LogicalCount() int {
	if v == nil || v.records == nil {
		return 0
	}
	return len(v.records.locations)
}

// ScanWithIDs visits logical IDs (including copies) in ascending order and
// passes each ID with its decoded row to yield. It mirrors Scan with one
// decoded row at a time; the caller budgets the sorted identity index and each
// row's text explicitly. Returning an error from yield stops the walk and is
// propagated unchanged, so callers can bound their own output.
func (v *View) ScanWithIDs(ctx context.Context, indexBytes int64, textBytes int, yield func(uint32, map[string]any) error) error {
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
		if err := ctx.Err(); err != nil {
			return err
		}
		row, err := v.Row(ctx, id, textBytes)
		if err != nil {
			return err
		}
		if err := yield(id, row); err != nil {
			return err
		}
	}
	return ctx.Err()
}
