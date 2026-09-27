package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// SourceQueryReading is a captured source query result.
type SourceQueryReading struct {
	Result  SearchResponse      `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

func pinnedSource(ctx context.Context, root, snapshot string) (selection.SourcePin, error) {
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, m *vault.Metadata) (selection.SourcePin, error) {
		set, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return selection.SourcePin{}, err
		}
		if set.Source == nil {
			return selection.SourcePin{}, errors.New("codebase.source_pin_required")
		}
		return *set.Source, nil
	})
}

func captureSourceJSON(ctx context.Context, root string, value any, complete, truncated bool, provenance evidence.Provenance) (evidence.CaptureRef, error) {
	raw, err := json.Marshal(value)
	if err != nil {
		return evidence.CaptureRef{}, err
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json", Complete: complete, Truncated: truncated, Provenance: provenance})
	})
}

// QuerySource runs one documented search-mode query against the fixed
// snapshot and archives the exact result as evidence.
func QuerySource(ctx context.Context, root, snapshot string, query SearchQuery) (SourceQueryReading, error) {
	var result SourceQueryReading
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return result, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	b := OpenBrowser(s)
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return result, err
	}
	result.Result, err = b.Search(ctx, snapshot, pin, query)
	if err != nil {
		return result, err
	}
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, result.Result.Complete, result.Result.Truncated, evidence.Provenance{Kind: "source-query", Locator: query.Text, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return result, err
}

// TargetReading is a captured symbol/path inspection result.
type TargetReading struct {
	Result  SearchResponse      `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

// InspectSourceTarget inspects one symbol or path of the fixed snapshot.
func InspectSourceTarget(ctx context.Context, root, snapshot string, query TargetQuery) (TargetReading, error) {
	var result TargetReading
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return result, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	b := OpenBrowser(s)
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return result, err
	}
	result.Result, err = b.InspectTarget(ctx, snapshot, pin, query)
	if err != nil {
		return result, err
	}
	target := query.Symbol
	if target == "" {
		target = query.Path
	}
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, result.Result.Complete, result.Result.Truncated, evidence.Provenance{Kind: "source-inspect", Locator: target, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return result, err
}

// SourceValidation is a captured TOC validation result.
type SourceValidation struct {
	Result  TOCValidation       `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

// ValidateSourceTOC runs the TOC-closure validation mode against the fixed
// snapshot. There is no latest fallback: evidence is exactly the pinned commit.
func ValidateSourceTOC(ctx context.Context, root, snapshot string, input AddonInput) (SourceValidation, error) {
	var result SourceValidation
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return result, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	b := OpenBrowser(s)
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return result, err
	}
	result.Result, err = b.ValidateTOC(ctx, pin, snapshot, input, ValidationIdentity{})
	if err != nil {
		return result, err
	}
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, result.Result.Valid, false, evidence.Provenance{Kind: "addon-validate", Locator: input.Root + "/" + input.Manifest, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return result, err
}

// MatrixValidation is a captured merged matrix validation result.
type MatrixValidation struct {
	Result  MatrixResult        `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

// ValidateSourceMatrix strictly reads one matrix file, resolves each target to
// fixed evidence (never a moving ref), validates every TOC closure against its
// own pinned index and merges diagnostics while keeping per-target identity.
// With a nil resolver, refs are prepared through the repository catalog.
func ValidateSourceMatrix(ctx context.Context, root, matrixFile string, resolve TargetResolver, options ...ValidationOptions) (MatrixValidation, error) {
	config, addonRoot, err := ReadMatrixConfig(matrixFile)
	if err != nil {
		return MatrixValidation{}, err
	}
	var result MatrixValidation
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	browser := OpenBrowser(s)
	targets := make([]TOCValidation, 0, len(config.Targets))
	for _, target := range config.Targets {
		repository := target.Source
		if repository == "" {
			repository = DefaultMatrixSource
		}
		var pin selection.SourcePin
		if resolve != nil {
			pin, err = resolve(ctx, repository, target.Product, target.Ref)
		} else {
			pin, err = browser.PrepareSource(ctx, repository, target.Product, target.Ref)
		}
		if err != nil {
			return result, err
		}
		if err := browser.EnsureIndex(ctx, pin); err != nil {
			return result, err
		}
		value, err := browser.ValidateTOC(ctx, pin, "", AddonInput{Root: addonRoot, Manifest: target.TOC}, ValidationIdentity{ID: target.ID, Ref: target.Ref})
		if err != nil {
			return result, err
		}
		if len(options) > 0 && options[0].Semantic != nil {
			value.Semantic, err = browser.checkSemantic(ctx, pin, value.load, options[0].Semantic, options[0].EnvironmentSnapshot)
			if err != nil {
				return result, err
			}
			value.Valid = value.Valid && value.Semantic.Passed
		}
		targets = append(targets, value)
	}
	result.Result = MergeMatrix(filepath.Clean(addonRoot), targets)
	result.Capture, err = captureSourceJSON(ctx, root, result.Result, result.Result.Valid, false, evidence.Provenance{Kind: "addon-matrix-validate", Locator: matrixFile})
	return result, err
}

// SourceSnapshotStatus reports prepared-snapshot readiness for one pinned set.
func SourceSnapshotStatus(ctx context.Context, root, snapshot string) (SnapshotStatus, error) {
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (SnapshotStatus, error) {
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return SnapshotStatus{}, err
		}
		if pin.Source == nil {
			return SnapshotStatus{}, errors.New("codebase.source_pin_required")
		}
		return OpenBrowser(s).SnapshotStatus(ctx, *pin.Source)
	})
}

// FixtureIndexResult carries the deterministic fixture pin and its index.
type FixtureIndexResult struct {
	Pin      selection.SourcePin `json:"pin"`
	Snapshot selection.PinnedSet `json:"snapshot"`
	Summary  IndexSummary        `json:"summary"`
}

// IndexFixtureSource indexes a local fixture directory under its deterministic
// synthetic commit and pins that identity, for tests and offline fixtures.
func IndexFixtureSource(ctx context.Context, root, repository, product, sourcePath string) (FixtureIndexResult, error) {
	absolute, err := filepath.Abs(sourcePath)
	if err != nil {
		return FixtureIndexResult{}, err
	}
	pin, err := FixtureSourcePin(repository, product, absolute)
	if err != nil {
		return FixtureIndexResult{}, err
	}
	result := FixtureIndexResult{Pin: pin}
	s, err := vault.OpenStore(root)
	if err != nil {
		return result, err
	}
	result.Summary, err = OpenBrowser(s).IndexFixture(ctx, pin, absolute)
	if err != nil {
		return result, err
	}
	result.Snapshot, err = vault.WriteMetadata(ctx, root, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
		return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
	})
	return result, err
}

// CheckSourceMirror reports mirror health for one catalog repository product.
// With offline true it performs zero network requests.
func CheckSourceMirror(ctx context.Context, home, repository, product string, offline bool) (MirrorHealth, error) {
	repo, err := LookupRepository(repository)
	if err != nil {
		return MirrorHealth{}, err
	}
	return CheckMirrorHealth(ctx, home, repo, product, offline)
}
