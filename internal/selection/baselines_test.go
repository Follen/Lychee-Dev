package selection

import (
	"strings"
	"testing"
)

func TestSourceInterfaceRequiresExactApprovedIdentity(t *testing.T) {
	want := map[string]int{"retail": 120100, "classic": 50504, "titan": 38002, "forever": 16001}
	for _, baseline := range VerifiedClientBaselines() {
		pin := SourcePin{Repository: "wow-ui-source", Product: baseline.Product, ExactCommit: baseline.SourceCommit}
		value, ok := SourceInterface(pin)
		if !ok || value != want[baseline.Product] {
			t.Fatalf("%+v: %d %v", baseline, value, ok)
		}
		pin.ExactCommit = strings.Repeat("a", 40)
		if _, ok := SourceInterface(pin); ok {
			t.Fatal("unverified commit accepted")
		}
		pin.ExactCommit = baseline.SourceCommit
		pin.Repository = "elvui"
		if _, ok := SourceInterface(pin); ok {
			t.Fatal("addon version treated as game Interface")
		}
	}
	copy := VerifiedClientBaselines()
	copy[0].Interface = 0
	if VerifiedClientBaselines()[0].Interface == 0 {
		t.Fatal("catalog mutated through return value")
	}
}
