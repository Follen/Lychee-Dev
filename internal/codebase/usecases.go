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

// QuerySource runs one documented search-mode query against the fixed
// snapshot and archives the exact result as evidence.
func QuerySource(ctx context.Context, root, snapshot string, query SearchQuery) (SourceQueryReading, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (SourceQueryReading, error) {
		var result SourceQueryReading
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenBrowser(s).Search(ctx, snapshot, *pin.Source, query)
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json",
			Complete: result.Result.Complete, Truncated: result.Result.Truncated,
			Provenance: evidence.Provenance{Kind: "source-query", Locator: query.Text, Snapshot: snapshot, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
}

// TargetReading is a captured symbol/path inspection result.
type TargetReading struct {
	Result  SearchResponse      `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

// InspectSourceTarget inspects one symbol or path of the fixed snapshot.
func InspectSourceTarget(ctx context.Context, root, snapshot string, query TargetQuery) (TargetReading, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (TargetReading, error) {
		var result TargetReading
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenBrowser(s).InspectTarget(ctx, snapshot, *pin.Source, query)
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		target := query.Symbol
		if target == "" {
			target = query.Path
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json",
			Complete: result.Result.Complete, Truncated: result.Result.Truncated,
			Provenance: evidence.Provenance{Kind: "source-inspect", Locator: target, Snapshot: snapshot, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
}

// SourceValidation is a captured TOC validation result.
type SourceValidation struct {
	Result  TOCValidation       `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

// ValidateSourceTOC runs the TOC-closure validation mode against the fixed
// snapshot. There is no latest fallback: evidence is exactly the pinned commit.
func ValidateSourceTOC(ctx context.Context, root, snapshot string, input AddonInput) (SourceValidation, error) {
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (SourceValidation, error) {
		var result SourceValidation
		pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return result, err
		}
		if pin.Source == nil {
			return result, errors.New("codebase.source_pin_required")
		}
		result.Result, err = OpenBrowser(s).ValidateTOC(ctx, *pin.Source, snapshot, input, ValidationIdentity{})
		if err != nil {
			return result, err
		}
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json",
			Complete: result.Result.Valid, Truncated: false,
			Provenance: evidence.Provenance{Kind: "addon-validate", Locator: input.Root + "/" + input.Manifest, Snapshot: snapshot, SourceCommit: pin.Source.ExactCommit},
		})
		return result, err
	})
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
func ValidateSourceMatrix(ctx context.Context, root, matrixFile string, resolve TargetResolver) (MatrixValidation, error) {
	config, addonRoot, err := ReadMatrixConfig(matrixFile)
	if err != nil {
		return MatrixValidation{}, err
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (MatrixValidation, error) {
		var result MatrixValidation
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
			// Fixed evidence only: use an existing published index or prepare
			// this exact commit once. Nothing resolves a moving ref afterwards.
			if db, _, openErr := browser.openIndex(ctx, pin); openErr == nil {
				db.Close()
			} else if errors.Is(openErr, errIndexNotReady) {
				if _, err := browser.IndexSource(ctx, pin); err != nil {
					return result, err
				}
			} else {
				return result, openErr
			}
			value, err := browser.ValidateTOC(ctx, pin, "", AddonInput{Root: addonRoot, Manifest: target.TOC}, ValidationIdentity{ID: target.ID, Ref: target.Ref})
			if err != nil {
				return result, err
			}
			targets = append(targets, value)
		}
		result.Result = MergeMatrix(filepath.Clean(addonRoot), targets)
		raw, err := json.Marshal(result.Result)
		if err != nil {
			return result, err
		}
		result.Capture, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: 16 << 20, MediaType: "application/json",
			Complete: result.Result.Valid, Truncated: false,
			Provenance: evidence.Provenance{Kind: "addon-matrix-validate", Locator: matrixFile},
		})
		return result, err
	})
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
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (FixtureIndexResult, error) {
		var result FixtureIndexResult
		result.Pin = pin
		result.Summary, err = OpenBrowser(s).IndexFixture(ctx, pin, absolute)
		if err != nil {
			return result, err
		}
		result.Snapshot, err = selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
		return result, err
	})
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
