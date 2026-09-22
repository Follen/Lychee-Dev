package selection

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"
)

// ErrTargetBuildUnavailable is the precise failure for an explicit build
// constraint that cannot be satisfied right now.
var ErrTargetBuildUnavailable = errors.New("selection.target_build_unavailable")

// ErrTargetRequired reports that no explicit snapshot, target or project lock
// was supplied and no default may be invented.
var ErrTargetRequired = errors.New("selection.target_required")

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

// IntentRequest expresses the caller's selection request. Priority is fixed:
// explicit pinned snapshot, then explicit named target configuration, then the
// project lock. Only supplied layers are considered; nothing is invented.
type IntentRequest struct {
	Snapshot    string
	Target      string
	Project     string
	Preparer    TargetPreparer
	Parent      string
	Definitions string
	Offline     bool
}

// IntentChoice is the resolved choice plus honest conflict notes about the
// lower-priority layers that were supplied and therefore superseded.
type IntentChoice struct {
	Origin     string    `json:"origin"` // "snapshot" | "target" | "project-lock"
	Pin        PinnedSet `json:"pin"`
	TargetName string    `json:"targetName,omitempty"`
	Conflicts  []string  `json:"conflicts,omitempty"`
}

// ResolveIntent applies the fixed selection order and reports conflicts
// instead of silently overwriting a lower-priority layer.
func ResolveIntent(ctx context.Context, root string, request IntentRequest) (IntentChoice, error) {
	var choice IntentChoice
	// Read the project lock cheaply first so conflicts can be reported even
	// when a higher-priority layer wins.
	var projectStatus *ProjectStatus
	if request.Project != "" {
		status, err := InspectProject(ctx, request.Project)
		if err != nil && !errors.Is(err, ErrProjectMissing) {
			return choice, err
		}
		if err == nil {
			projectStatus = &status
		}
	}

	switch {
	case request.Snapshot != "":
		pin, err := InspectSelection(ctx, root, request.Snapshot)
		if err != nil {
			return choice, err
		}
		choice = IntentChoice{Origin: "snapshot", Pin: pin}
		if request.Target != "" {
			choice.Conflicts = append(choice.Conflicts,
				fmt.Sprintf("named target %q superseded by the explicit snapshot %s", request.Target, pin.ID))
		}
		if projectStatus != nil && projectStatus.Lock != nil {
			if conflict := pinConflictNote("project lock", projectStatus.Lock.Selection, pin); conflict != "" {
				choice.Conflicts = append(choice.Conflicts, conflict)
			}
		}
		return choice, nil

	case request.Target != "":
		resolved, err := ResolveTarget(ctx, root, request.Target, ResolveOptions{
			Preparer: request.Preparer, Parent: request.Parent, Definitions: request.Definitions, Offline: request.Offline,
		})
		if err != nil {
			return choice, err
		}
		choice = IntentChoice{Origin: "target", Pin: resolved.Pin, TargetName: resolved.Config.Name}
		if projectStatus != nil && projectStatus.Lock != nil {
			if conflict := pinConflictNote("project lock", projectStatus.Lock.Selection, resolved.Pin); conflict != "" {
				choice.Conflicts = append(choice.Conflicts, conflict)
			}
		}
		return choice, nil

	case projectStatus != nil && projectStatus.Lock != nil:
		status, err := LoadProjectSelection(ctx, root, projectStatus.Directory)
		if err != nil {
			return choice, err
		}
		if status.Lock == nil {
			return choice, ErrProjectUnlocked
		}
		return IntentChoice{Origin: "project-lock", Pin: status.Lock.Selection}, nil

	default:
		return choice, ErrTargetRequired
	}
}

func pinConflictNote(layer string, layerPin, chosen PinnedSet) string {
	if layerPin.ID == chosen.ID {
		return ""
	}
	layerProduct, chosenProduct := pinProduct(layerPin), pinProduct(chosen)
	if layerProduct == chosenProduct {
		return fmt.Sprintf("%s %s superseded by the explicit selection %s (same product %s)",
			layer, layerPin.ID, chosen.ID, chosenProduct)
	}
	return fmt.Sprintf("%s %s (product %s) conflicts with the explicit selection %s (product %s) and was superseded",
		layer, layerPin.ID, layerProduct, chosen.ID, chosenProduct)
}

// TargetSummary renders one configuration line for text listings.
func TargetSummary(config TargetConfig) string {
	parts := []string{config.Name, config.Product, config.Region, config.Locale}
	if config.FullBuild != "" {
		parts = append(parts, config.FullBuild)
	}
	parts = append(parts, config.Source)
	if config.Installation != "" {
		parts = append(parts, config.Installation)
	}
	if config.Repository != "" {
		parts = append(parts, "repository="+config.Repository)
	}
	return strings.Join(parts, " ")
}
