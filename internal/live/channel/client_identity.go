package channel

import (
	"strings"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/selection"
)

// sameConnectionTarget preserves the identity checks on persisted connections.
// Before catalogProduct was recorded, a reusable slot could only be the
// baseline's original DataSlot. Additional regional slots are never inferred
// for old records. Normalize a copy, keeping read-only recovery independent of
// metadata migration and leaving every other client/window field exact.
func sameConnectionTarget(saved, observed live.ClientWindow) bool {
	if saved == observed {
		return true
	}
	if saved.Client.CatalogProduct != "" || observed.Client.CatalogProduct == "" {
		return false
	}
	for _, baseline := range selection.VerifiedClientBaselines() {
		if saved.Client.Product == baseline.Product && saved.Client.ProductCode == baseline.ProductCode &&
			baseline.DataSlot != "" && baseline.DataSlot != baseline.ProductCode &&
			observed.Client.CatalogProduct == baseline.DataSlot &&
			strings.HasPrefix(saved.Client.FullBuild, baseline.BuildSeries+".") {
			saved.Client.CatalogProduct = baseline.DataSlot
			return saved == observed
		}
	}
	return false
}
