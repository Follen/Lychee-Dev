package selection

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"time"
)

// ErrTargetBuildUnavailable is the precise failure for an explicit build
// constraint that cannot be satisfied right now.
var ErrTargetBuildUnavailable = errors.New("selection.target_build_unavailable")

// PrepareOptions carries the caller's immutable extensions and offline mode
// into target preparation.
type PrepareOptions struct {
	Parent      string
	Definitions string
	Offline     bool
}

// TargetPreparer resolves one mutable target configuration into a fresh
// immutable identity through the existing preparation modules (local
// installation preparation or remote release identity preparation). The
// production adapter wraps the records use cases; tests provide fakes. An
// adapter must translate "manifest lacks this exact build" and "the CDN no
// longer serves its configurations" into ErrTargetBuildUnavailable so callers
// see one precise code.
//
// Historical remote build resolution rule (closes the old "historical remote
// build source" gap without inventing a third-party historical source):
//
//   - Without an exact FullBuild the named target resolves to the region's
//     current release row from the versions manifest ("latest"). When that row
//     later moves, a NEW resolution legitimately yields a different pinned set;
//     an existing pinned set is never re-resolved.
//   - With an exact FullBuild the target resolves ONLY when the versions
//     manifest still lists that exact build for the region AND the CDN still
//     serves its build and CDN configurations. Missing manifest row or missing
//     CDN configuration both fail with selection.target_build_unavailable.
//     Historical identity is never borrowed from another region, another
//     product, a cache of unrelated observations, or a third-party registry.
type TargetPreparer interface {
	PrepareTarget(ctx context.Context, workspace string, config TargetConfig, options PrepareOptions) (PinnedSet, error)
}

// ResolvedTarget is one command-start resolution: the mutable intent at that
// instant plus the fresh immutable identity it produced.
type ResolvedTarget struct {
	Config     TargetConfig `json:"config"`
	Pin        PinnedSet    `json:"pin"`
	ResolvedAt time.Time    `json:"resolvedAt"`
}

// ResolveOptions parameterizes one resolution.
type ResolveOptions struct {
	Preparer TargetPreparer
	Parent   string
	// Definitions overrides the configuration's definition reference.
	Definitions string
	Offline     bool
}

// ResolveTarget resolves a named configuration into a fresh PinnedSet through
// the supplied preparer at command start. Every call produces its own identity:
// concurrent resolutions never share or leak pins, and resolving twice may
// legitimately differ when latest moved. Successful resolutions are recorded
// in the target's reference ledger.
func ResolveTarget(ctx context.Context, root, key string, options ResolveOptions) (ResolvedTarget, error) {
	if options.Preparer == nil {
		return ResolvedTarget{}, errors.New("selection: missing target preparer")
	}
	config, err := LookupTarget(ctx, root, key)
	if err != nil {
		return ResolvedTarget{}, err
	}
	if config.Source == TargetSourceInstallation && !filepath.IsAbs(config.Installation) {
		return ResolvedTarget{}, fmt.Errorf("%w: target %s has an old relative installation path; replace it with an explicit installation path", ErrTargetFormat, config.Name)
	}
	definitions := options.Definitions
	if definitions == "" {
		definitions = config.Definitions
	}
	pin, err := options.Preparer.PrepareTarget(ctx, root, config, PrepareOptions{
		Parent: options.Parent, Definitions: definitions, Offline: options.Offline,
	})
	if err != nil {
		if errors.Is(err, ErrTargetBuildUnavailable) {
			return ResolvedTarget{}, fmt.Errorf("%w: %s (%v)", ErrTargetBuildUnavailable, config.FullBuild, err)
		}
		return ResolvedTarget{}, fmt.Errorf("selection.target_prepare_failed: %w", err)
	}
	if pin.ID == "" {
		return ResolvedTarget{}, errors.New("selection.target_prepare_failed: preparer returned no identity")
	}
	resolved := ResolvedTarget{Config: config, Pin: pin, ResolvedAt: time.Now().UTC()}
	if err := recordTargetReference(ctx, root, config.Name, TargetReference{
		Schema: TargetReferenceSchema, Name: config.Name, Kind: "pin", ID: pin.ID, CreatedAt: resolved.ResolvedAt,
	}); err != nil {
		return ResolvedTarget{}, err
	}
	return resolved, nil
}
