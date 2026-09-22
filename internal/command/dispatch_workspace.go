package command

import (
	"context"
	"errors"
	"fmt"
	"os"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Dispatch bodies for the workspace-management verbs: named targets, managed
// cache, workspace configuration, doctor aggregation and evidence listing.
// Every helper returns the argument-shape exit code (2) for CLI-side validation
// failures and lets module errors flow through the shared fault table.

// runTargetVerb dispatches named-target management and the remote availability
// listing. The named-target use cases validate their own configuration; only
// the flags the parser cannot know (supported product, region and locale pairs)
// are checked here so user input errors map to exit 2 instead of exit 5.
func runTargetVerb(ctx context.Context, route, argument string, opts Options, response *Envelope) (int, error) {
	switch route {
	case "target list":
		root, err := workspaceRoot(opts.home)
		if err != nil {
			return 0, err
		}
		configs, err := selection.ListTargets(ctx, root)
		if err != nil {
			return 0, err
		}
		response.Result = map[string]any{"targets": configs}
		return 0, nil
	case "target add":
		if argument == "" {
			return 2, errors.New("target add requires a name argument")
		}
		if opts.product == "" || opts.dataRegion == "" || opts.locale == "" {
			return 2, errors.New("target add requires --product, --region, and --locale")
		}
		if _, err := selection.DataProduct(opts.product); err != nil {
			return 2, errors.New("target add requires --product <retail|classic|titan|forever>")
		}
		if _, err := selection.DataLocale(opts.dataRegion, opts.locale); err != nil {
			return 2, errors.New("target add requires a supported --region and --locale pair")
		}
		config := selection.TargetConfig{
			Name: argument, Product: opts.product, Region: opts.dataRegion, Locale: opts.locale,
			FullBuild: opts.fullBuild, Installation: opts.installation,
		}
		if opts.remote {
			config.Source = selection.TargetSourceRemote
		} else {
			config.Source = selection.TargetSourceInstallation
		}
		root, err := workspaceRoot(opts.home)
		if err != nil {
			return 0, err
		}
		stored, err := selection.PutTarget(ctx, root, config, selection.StoreTargetOptions{Replace: opts.replace})
		if err != nil {
			return 0, err
		}
		response.Result = stored
		return 0, nil
	case "target remove":
		if argument == "" {
			return 2, errors.New("target remove requires a name argument")
		}
		root, err := workspaceRoot(opts.home)
		if err != nil {
			return 0, err
		}
		report, err := selection.RemoveTarget(ctx, root, argument)
		if err != nil {
			return 0, err
		}
		response.Result = report
		return 0, nil
	case "target available":
		if opts.dataRegion == "" {
			return 2, errors.New("target available requires --region")
		}
		if _, err := selection.DataLocale(opts.dataRegion, "enUS"); err != nil {
			return 2, errors.New("target available requires --region <us|eu|cn|kr|tw>")
		}
		availability, err := selection.ListRemoteAvailability(ctx, selection.AvailabilityOptions{Region: opts.dataRegion, Offline: opts.offline})
		if err != nil {
			return 0, err
		}
		for _, product := range availability.Products {
			if product.Error != "" {
				response.Warnings = append(response.Warnings, "At least one product manifest could not be read; inspect products[].error for the exact failure.")
				break
			}
		}
		response.Result = availability
		return 0, nil
	}
	return 2, fmt.Errorf("unknown command %q; use --help", route)
}

// runCacheVerb dispatches managed-cache accounting, verification and reclaim.
// Verification failure keeps the report in the result and maps to the
// integrity fault, mirroring how source validate reports failed static checks.
func runCacheVerb(ctx context.Context, route string, opts Options, response *Envelope) (int, error) {
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		return 0, err
	}
	cache := store.Cache()
	switch route {
	case "cache status":
		status, err := cache.Status(ctx)
		if err != nil {
			return 0, err
		}
		response.Result = status
	case "cache verify":
		report, err := cache.Verify(ctx)
		if err != nil {
			return 0, err
		}
		response.Result = report
		if !report.OK {
			response.Warnings = append(response.Warnings, "Managed cache integrity failed; inspect missing and corrupt lists and decide on an explicit prune. Nothing was repaired or deleted.")
			return 0, vault.ErrCacheIntegrity
		}
	case "cache prune":
		policy := vault.PrunePolicy{TargetBytes: opts.targetBytes, MaxObjects: opts.maxObjects, IncludeUncommitted: opts.uncommitted, DryRun: opts.dryRun}
		report, err := cache.Prune(ctx, policy)
		if err != nil {
			return 0, err
		}
		response.Result = report
		if policy.DryRun {
			response.Warnings = append(response.Warnings, "Dry run: nothing was deleted.")
		}
		if !report.Complete {
			response.Warnings = append(response.Warnings, "This prune stopped before reaching its target; inspect truncated and the skipped counters before rerunning.")
		}
	}
	return 0, nil
}

// runConfigVerb reads or mutates the workspace budget document. The mutation
// use case validates the resulting document and refuses newer or corrupt
// schemas; both map through the shared fault table.
func runConfigVerb(ctx context.Context, route string, opts Options, response *Envelope) (int, error) {
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	if route == "config show" {
		config, err := vault.ReadConfig(ctx, root)
		if err != nil {
			return 0, err
		}
		response.Result = config
		return 0, nil
	}
	config, err := vault.UpdateConfig(ctx, root, func(current *vault.Config) error {
		if opts.cacheMaxBytesSet {
			current.Cache.MaxBytes = opts.cacheMaxBytes
		}
		if opts.downloadWorkersSet {
			current.Download.Workers = opts.downloadWorkers
		}
		return nil
	})
	if err != nil {
		return 0, err
	}
	response.Result = config
	return 0, nil
}

// runDoctor aggregates the independent read-only health checks. Exit 3 is
// reserved for checks with error status: warn results (a brand new workspace
// without a metadata database yet, stale temporary files) describe degraded
// but working states and stay at exit 0 with the check visible.
func runDoctor(ctx context.Context, opts Options, response *Envelope) (int, error) {
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	userHome, err := userHomeDirectory()
	if err != nil {
		return 0, err
	}
	checks := vault.WorkspaceChecks(ctx, root, vault.LegacyRootCandidates(userHome, os.Getenv("LOCALAPPDATA")))
	checks = append(checks, selection.SelectionChecks(ctx, root)...)
	// The mirror check joins only when the nearest project declares a product;
	// doctor never invents a repository or a product default.
	if directory, projectErr := projectDirectory(""); projectErr == nil {
		if status, statusErr := selection.InspectProject(ctx, directory); statusErr == nil && status.Project.Product != "" {
			checks = append(checks, sourceMirrorCheck(ctx, root, status.Project.Product, opts.offline))
		}
	}
	failed := false
	for _, check := range checks {
		if check.Status == "error" {
			failed = true
		}
	}
	response.Context["workspace"] = root
	response.Result = map[string]any{"healthy": !failed, "checks": checks}
	if failed {
		return 0, errDoctorUnhealthy
	}
	return 0, nil
}

// sourceMirrorCheck converts one bounded mirror probe into the shared check
// shape. A mirror that was never synchronized is a normal fresh-workspace
// state, so unknown remote currency never fails doctor.
func sourceMirrorCheck(ctx context.Context, root, product string, offline bool) vault.Check {
	health, err := codebase.CheckSourceMirror(ctx, root, "wow-ui-source", product, offline)
	check := vault.Check{ID: "codebase.source_mirror", OK: true, NextStep: "run source sync --source wow-ui-source --product " + product + " to prepare or refresh the mirror"}
	if err != nil {
		check.Status = "unknown"
		check.Detail = "mirror health probe failed: " + err.Error()
		return check
	}
	detail := "mirror for " + product + " is not initialized yet"
	if health.Initialized {
		detail = "mirror head " + health.LocalCommit
	}
	switch health.RemoteStatus {
	case "checked":
		check.Detail = detail + "; remote head " + health.RemoteCommit
		if health.UpdateAvailable {
			check.Detail += "; update available"
		}
	case "skipped_offline":
		check.Status = "unknown"
		check.Detail = detail + "; remote currency not checked (--offline)"
	default:
		check.Status = "unknown"
		check.Detail = detail + "; remote currency unknown"
	}
	return check
}

// runEvidenceList pages the archived capture manifests through the existing
// evidence archive helper. Listing is bounded by --limit and never reads
// capture payload bytes.
func runEvidenceList(ctx context.Context, opts Options, response *Envelope) (int, error) {
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	captures, err := vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) ([]evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).ListCaptures(ctx, "", opts.limit)
	})
	if err != nil {
		return 0, err
	}
	response.Result = map[string]any{"captures": captures}
	return 0, nil
}
