package codebase

import (
	"bytes"
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSemanticDefinitionsReuseFixedEnvironment(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := IndexFixtureSource(ctx, root, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(s)
	definitions, coverage, err := b.semanticDefinitions(ctx, indexed.Pin, "")
	if err != nil {
		t.Fatal(err)
	}
	environment, state, err := b.researchEnvironment(ctx, indexed.Pin, "")
	if err != nil || state != "ready" || !bytes.Equal(definitions, environment.Definitions) || coverage.Documents != environment.Coverage.Documents {
		t.Fatalf("semantic definitions diverged from fixed environment: state=%s coverage=%+v err=%v", state, coverage, err)
	}
	if !bytes.Contains(definitions, []byte("C_AuctionHouse")) {
		t.Fatalf("fixture API missing from definitions: %s", definitions)
	}
	thirdParty := indexed.Pin
	thirdParty.Repository = "third-party"
	if _, _, err := b.semanticDefinitions(ctx, thirdParty, ""); !errors.Is(err, luals.ErrUnavailable) {
		t.Fatalf("third-party API assumed without fixed client pin: %v", err)
	}
}

func TestSemanticDependentCapturesVerifyAfterCachePrune(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	input := []byte("local amount=UnitHealth('player')\n")
	ref, err := s.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(input), MaxBytes: 1024})
	if err != nil {
		t.Fatal(err)
	}
	result := &SemanticAssessment{Inputs: []SemanticInput{{Path: "Addon/Main.lua", Blob: ref}}, Artifacts: map[string]vault.BlobRef{}, Captures: map[string]evidence.CaptureRef{}}
	result.Config = []byte(`{"runtime":"pinned"}`)
	result.Raw = []byte(`{"diagnostics":[]}`)
	b := OpenBrowser(s)
	if err := b.registerSemanticEvidence(ctx, fixtureSourcePinForSemantic(), result, map[string][]byte{"Addon/Main.lua": input}, []byte("---@meta\n")); err != nil {
		t.Fatal(err)
	}
	if result.Inputs[0].Capture == nil || len(result.Captures) != 3 {
		t.Fatalf("dependent captures missing: %+v", result)
	}
	if _, err := s.Cache().Put(ctx, vault.CacheInput{Reader: bytes.NewReader([]byte("ordinary cache")), MaxBytes: 1024}); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Cache().Prune(ctx, vault.PrunePolicy{TargetBytes: 0}); err != nil {
		t.Fatal(err)
	}
	for _, capture := range append([]evidence.CaptureRef{*result.Inputs[0].Capture}, result.Captures["definitions"], result.Captures["configuration"], result.Captures["rawReport"]) {
		_, err := vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (struct{}, error) {
			return struct{}{}, evidence.OpenArchive(s, m).VerifyCapture(ctx, capture.ID)
		})
		if err != nil {
			t.Fatalf("dependent capture %s lost after prune: %v", capture.ID, err)
		}
	}
}

func fixtureSourcePinForSemantic() selection.SourcePin {
	return selection.SourcePin{Repository: "wow-ui-source", Product: "retail", ExactCommit: "fixed-fixture"}
}
