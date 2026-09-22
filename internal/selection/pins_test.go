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
