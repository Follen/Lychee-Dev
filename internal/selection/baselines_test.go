package selection

import (
	"strings"
	"testing"
)

func TestSourceInterfaceRequiresExactApprovedIdentity(t *testing.T) {
	want := map[string]int{"retail/12.1.5": 120105, "retail/12.1.0": 120100, "classic/5.5.4": 50504, "classic-era/1.15.9": 11509, "anniversary/2.5.6": 20506, "titan/3.80.2": 38002, "forever/1.60.1": 16001}
	for _, baseline := range VerifiedClientBaselines() {
		pin := SourcePin{Repository: "wow-ui-source", Product: baseline.Product, ExactCommit: baseline.SourceCommit}
		value, ok := SourceInterface(pin)
		if !ok || value != want[baseline.Product+"/"+baseline.BuildSeries] {
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
