package channel

import (
	"strings"

	"github.com/follenfang/lycheedev/internal/live"
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
	if saved.Client.Product == "forever" && saved.Client.ProductCode == "wow_forever" &&
		observed.Client.CatalogProduct == "wow_classic_beta" && strings.HasPrefix(saved.Client.FullBuild, "1.60.1.") {
		saved.Client.CatalogProduct = "wow_classic_beta"
		return saved == observed
	}
	return false
}
