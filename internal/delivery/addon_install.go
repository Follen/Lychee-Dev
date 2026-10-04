package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

// InstallAddon publishes only a validated, independent release snapshot. Existing
// unmanaged addons, including legacy task registries, are never adopted.
func InstallAddon(ctx context.Context, releaseDirectory, clientDirectory, version string) (receipt InstallationReceipt, err error) {
	return withAddonRelease(ctx, releaseDirectory, clientDirectory, version, func(prepared, target string) (InstallationReceipt, error) {
		current, inspectErr := InspectInstallation(ctx, target, "addon")
		if inspectErr != nil {
			return InstallationReceipt{}, inspectErr
		}
		if current.State == "managed" && current.Receipt != nil {
			if err := rejectLegacySlotsForInstall(ctx, filepath.Dir(target), current.Receipt.Version); err != nil {
				return InstallationReceipt{}, err
			}
		}
		return InstallFresh(ctx, prepared, target, "addon", version)
	})
}

func UpgradeAddon(ctx context.Context, releaseDirectory, clientDirectory, archive, version string, resume bool) (Upgrade, error) {
	if resume {
		_, parent, err := ResolveAddonDestination(ctx, clientDirectory)
		if err != nil {
			return Upgrade{}, err
		}
		gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
		if err != nil {
			return Upgrade{}, err
		}
		defer gate.Close()
		target := filepath.Join(parent, "Lychee Dev")
		if err = preflightResumeLegacySlots(ctx, parent, target, archive, version); err != nil {
			return Upgrade{}, err
		}
		upgrade, err := ResumeUpgrade(ctx, target, archive, "addon")
		if err == nil {
			previous, previousErr := InspectInstallation(ctx, filepath.Join(archive, "previous"), "addon")
			if previousErr != nil {
				err = previousErr
			} else if previous.State == "managed" && previous.Receipt != nil {
				err = archiveLegacySlots(ctx, parent, archive+".slots", previous.Receipt.Version)
			}
		}
		return upgrade, err
	}
	return withAddonRelease(ctx, releaseDirectory, clientDirectory, version, func(prepared, target string) (Upgrade, error) {
		parent := filepath.Dir(target)
		current, inspectErr := InspectInstallation(ctx, target, "addon")
		if inspectErr != nil {
			return Upgrade{}, inspectErr
		}
		var oldVersion string
		if current.State == "managed" && current.Receipt != nil {
			oldVersion = current.Receipt.Version
			if _, err := preflightLegacySlots(ctx, parent, oldVersion, archive+".slots"); err != nil {
				return Upgrade{}, err
			}
		}
		upgrade, err := UpgradeInstallation(ctx, prepared, target, archive, "addon", version)
		if err == nil && oldVersion != "" {
			err = archiveLegacySlots(ctx, parent, archive+".slots", oldVersion)
		}
		return upgrade, err
	})
}

func preflightResumeLegacySlots(ctx context.Context, parent, target, archive, version string) error {
	marker := filepath.Join(parent, slotMarker)
	if _, err := os.Lstat(marker); errors.Is(err, os.ErrNotExist) {
		return rejectUnmanagedLegacySlots(parent)
	} else if err != nil {
		return err
	}
	current, err := InspectInstallation(ctx, target, "addon")
	if err != nil {
		return err
	}
	previous, err := InspectInstallation(ctx, filepath.Join(archive, "previous"), "addon")
	if err != nil {
		return err
	}
	oldVersion := ""
	if previous.State == "managed" && previous.Receipt != nil {
		oldVersion = previous.Receipt.Version
	} else if current.State == "managed" && current.Receipt != nil && current.Receipt.Version != version {
		oldVersion = current.Receipt.Version
	}
	if oldVersion == "" {
		return fmt.Errorf("%w: cannot bind legacy slot migration to the interrupted upgrade", ErrConflict)
	}
	archiveSlots := archive + ".slots"
	intentPath := archiveSlots + ".migration.json"
	if _, err = os.Lstat(intentPath); err == nil {
		return archiveLegacySlots(ctx, parent, archiveSlots, oldVersion)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	_, err = preflightLegacySlots(ctx, parent, oldVersion, archiveSlots)
	return err
}

func RemoveAddon(ctx context.Context, clientDirectory, archive string) (Removal, error) {
	_, parent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return Removal{}, err
	}
	gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
	if err != nil {
		return Removal{}, err
	}
	defer gate.Close()
	return removeAddonAndSlots(ctx, parent, archive)
}

// AddonDirectory is the single layout rule for the managed addon in a client.
// Callers still use ResolveAddonDestination or a queue operation to verify the
// installation and its ownership before touching files.
func AddonDirectory(clientDirectory string) string {
	return filepath.Join(clientDirectory, "Interface", "AddOns", "Lychee Dev")
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
	currentDeployment, currentParent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return receipt, err
	}
	if currentDeployment.Client != deployment.Client || currentParent != parent {
		return receipt, fmt.Errorf("%w: client identity changed during preparation", ErrConflict)
	}
	gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
	if err != nil {
		return receipt, err
	}
	defer func() { err = errors.Join(err, gate.Close()) }()
	installed, err := InspectInstallation(ctx, filepath.Join(parent, "Lychee Dev"), "addon")
	if err != nil {
		return receipt, err
	}
	if installed.State == "absent" {
		if err = rejectUnmanagedLegacySlots(parent); err != nil {
			return receipt, err
		}
	}
	receipt, err = publish(private, filepath.Join(parent, "Lychee Dev"))
	return receipt, err
}
