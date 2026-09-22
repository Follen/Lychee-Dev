package delivery_test

import (
	"github.com/follenfang/lycheedev/internal/delivery"
)

func sameAddonReceipt(a, b delivery.InstallationReceipt) bool {
	if a.Schema != b.Schema || a.Component != b.Component || a.Version != b.Version || a.Commit != b.Commit || len(a.Resources) != len(b.Resources) {
		return false
	}
	for i := range a.Resources {
		if a.Resources[i] != b.Resources[i] {
			return false
		}
	}
	return true
}
