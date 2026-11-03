package selection

import "testing"

func TestSourceLoadContextUsesProductIdentity(t *testing.T) {
	for _, tc := range []struct{ product, family, game string }{{"retail", "Mainline", "Standard"}, {"classic", "Classic", ""}, {"classic-era", "Classic", "Vanilla"}, {"anniversary", "Classic", ""}, {"titan", "Classic", ""}, {"forever", "Mainline", "Camelot"}, {"future", "", ""}} {
		family, game := SourceLoadContext(SourcePin{Repository: "wow-ui-source", Product: tc.product})
		if family != tc.family || game != tc.game {
			t.Fatalf("%s: %s/%s", tc.product, family, game)
		}
	}
	for _, baseline := range VerifiedClientBaselines() {
		if baseline.Product != "classic" && baseline.Product != "anniversary" {
			continue
		}
		_, game := SourceLoadContext(SourcePin{Repository: "wow-ui-source", Product: baseline.Product, ExactCommit: baseline.SourceCommit})
		if game == "" {
			t.Fatalf("current fixed baseline lacks game layout: %+v", baseline)
		}
	}
	if family, game := SourceLoadContext(SourcePin{Repository: "elvui", Product: "retail"}); family != "" || game != "" {
		t.Fatal("third-party track established client")
	}
}
