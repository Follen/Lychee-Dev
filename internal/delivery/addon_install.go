package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

// InstallAddon publishes only a validated, independent release snapshot. Existing
// unmanaged addons, including legacy task registries, are never adopted.
func InstallAddon(ctx context.Context, releaseDirectory, clientDirectory, version string) (receipt InstallationReceipt, err error) {
	return withAddonRelease(ctx, releaseDirectory, clientDirectory, version, func(prepared, target string) (InstallationReceipt, error) {
		return InstallFresh(ctx, prepared, target, "addon", version)
	})
}

func UpgradeAddon(ctx context.Context, releaseDirectory, clientDirectory, archive, version string, resume bool) (Upgrade, error) {
	if resume {
		_, parent, err := ResolveAddonDestination(ctx, clientDirectory)
		if err != nil {
			return Upgrade{}, err
		}
		return ResumeUpgrade(ctx, filepath.Join(parent, "Lychee Dev"), archive, "addon")
	}
	return withAddonRelease(ctx, releaseDirectory, clientDirectory, version, func(prepared, target string) (Upgrade, error) {
		return UpgradeInstallation(ctx, prepared, target, archive, "addon", version)
	})
}

func RemoveAddon(ctx context.Context, clientDirectory, archive string) (Removal, error) {
	_, parent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return Removal{}, err
	}
	return RemoveInstallation(ctx, filepath.Join(parent, "Lychee Dev"), archive, "addon")
}

func ResolveAddonDestination(ctx context.Context, clientDirectory string) (AddonDeployment, string, error) {
	deployment, err := InspectAddonDeployment(ctx, clientDirectory)
	if err != nil {
		return deployment, "", err
	}
	parent := filepath.Join(deployment.Client.Directory, "Interface", "AddOns")
	// Resolve each existing deployment parent without following redirects outside
	// the identified client. The delivery layer separately guards the final tree.
	for _, folder := range []string{filepath.Dir(parent), parent} {
		info, checkErr := os.Lstat(folder)
		if checkErr != nil {
			return deployment, "", checkErr
		}
		resolved, checkErr := filepath.EvalSymlinks(folder)
		if checkErr != nil {
			return deployment, "", checkErr
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || resolved != folder {
			return deployment, "", fmt.Errorf("%w: redirected addon parent", ErrInstallation)
		}
	}
	return deployment, parent, nil
}

func withAddonRelease[T any](ctx context.Context, releaseDirectory, clientDirectory, version string, publish func(string, string) (T, error)) (receipt T, err error) {
	deployment, parent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return receipt, err
	}
	release, err := InspectRelease(ctx, releaseDirectory, version)
	if err != nil {
		return receipt, err
	}
	private, err := os.MkdirTemp(parent, ".lycheedev-release-")
	if err != nil {
		return receipt, err
	}
	defer func() { err = errors.Join(err, os.RemoveAll(private)) }()
	stage, err := StagePayload(ctx, filepath.Join(releaseDirectory, "payload"), private, release.Resources)
	if err != nil {
		return receipt, err
	}
	if err := os.Rename(stage, filepath.Join(private, "payload")); err != nil {
		return receipt, err
	}
	manifest, err := json.Marshal(release)
	if err != nil {
		return receipt, err
	}
	if err := os.WriteFile(filepath.Join(private, "release.json"), manifest, 0600); err != nil {
		return receipt, err
	}
	if _, err := InspectAddonRelease(ctx, private, version); err != nil {
		return receipt, err
	}
	current, currentParent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return receipt, err
	}
	if current.Client != deployment.Client || currentParent != parent {
		return receipt, fmt.Errorf("%w: client identity changed during preparation", ErrConflict)
	}
	return publish(private, filepath.Join(parent, "Lychee Dev"))
}
