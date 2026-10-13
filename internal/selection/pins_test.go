package selection

import (
	"context"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"strings"
	"testing"
)

func TestPinsAreStableDerivedAndIsolated(t *testing.T) {
	ctx := context.Background()
	s, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "new"))
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	p := OpenPinner(m)
	source := &SourcePin{Repository: "fixture", Product: "retail", RequestedRef: "latest", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}
	one, err := p.PinSelection(ctx, SelectionSpec{Source: source})
	if err != nil {
		t.Fatal(err)
	}
	two, err := p.PinSelection(ctx, SelectionSpec{Source: source})
	if err != nil || two.ID != one.ID {
		t.Fatalf("unstable: %v", err)
	}
	source.ExactCommit = strings.Repeat("b", 40)
	if one.Source.ExactCommit != strings.Repeat("a", 40) {
		t.Fatal("caller pointer changed pinned result")
	}
	if _, err := p.PinSelection(ctx, SelectionSpec{Parent: one.ID, Source: source}); err == nil {
		t.Fatal("replaced fixed source")
	}
	data := &DataPin{Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875", BuildConfig: strings.Repeat("c", 32), CDNConfig: strings.Repeat("d", 32), DefinitionCommit: strings.Repeat("e", 40)}
	derived, err := p.PinSelection(ctx, SelectionSpec{Parent: one.ID, Data: data})
	if err != nil {
		t.Fatal(err)
	}
	if derived.ID == one.ID || derived.Source.ExactCommit != one.Source.ExactCommit || derived.Data == nil {
		t.Fatalf("bad derivation: %+v", derived)
	}
	original, err := p.ReadPinnedSet(ctx, one.ID)
	if err != nil || original.Data != nil {
		t.Fatalf("parent changed: %+v %v", original, err)
	}
}

func TestForeverSourceFamilyKeepsRegionalDataIdentity(t *testing.T) {
	source := &SourcePin{Repository: "wow-ui-source", Product: "forever", RequestedRef: "refs/heads/forever", ExactCommit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069", ParserRevision: "source-v1"}
	data := &DataPin{Region: "cn", Language: "zhCN", FullBuild: "1.60.1.70338", BuildConfig: strings.Repeat("c", 32), CDNConfig: strings.Repeat("d", 32), DefinitionCommit: strings.Repeat("e", 40)}
	for _, product := range []string{"forever-asia-beta", "forever-cn-beta"} {
		data.Product = product
		if err := validatePins(SelectionSpec{Source: source, Data: data}); err != nil {
			t.Fatal(err)
		}
		regional := *source
		regional.Product = product
		if value, ok := SourceInterface(regional); !ok || value != 16001 {
			t.Fatalf("shared exact source baseline lost: %s %d %v", product, value, ok)
		}
	}
	source.Product = "forever-asia-beta"
	if err := validatePins(SelectionSpec{Source: source, Data: data}); err == nil {
		t.Fatal("regional source pin silently changed to CN")
	}
	source.Product = "forever"
	data.Product = "classic-era"
	if err := validatePins(SelectionSpec{Source: source, Data: data}); err == nil {
		t.Fatal("unrelated API family accepted")
	}
}
