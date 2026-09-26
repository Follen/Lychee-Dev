package live

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
)

var ErrUpgradeInstallation = errors.New("live.upgrade_requires_current_managed_addon")
var ErrUpgradeChanged = errors.New("live.upgrade_installation_changed")

// A runtime/package mismatch grants no ordinary input. Reload alone can bridge
// from an archived old identity to the exact current managed installation.
func managedReloadUpgrade(ctx context.Context, installation, from string) (string, error) {
	if from == buildinfo.Version {
		return "", nil
	}
	status, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(installation), "addon")
	if err != nil {
		return "", err
	}
	if status.State != "managed" || status.Receipt == nil || status.Receipt.Version != buildinfo.Version {
		return "", ErrUpgradeInstallation
	}
	return status.Receipt.Commit, nil
}

func reconnectForReload(ctx context.Context, root, id string, io *liveIO) (*WindowSession, string, error) {
	bound, err := ReadWindowSession(ctx, root, id)
	if err != nil {
		return nil, "", err
	}
	if bound.Ready.Release == buildinfo.Version {
		return reconnectSession(ctx, root, id, io)
	}
	if _, err := managedReloadUpgrade(ctx, bound.Target.Client.Directory, bound.Ready.Release); err != nil {
		return nil, "", err
	}
	session, snapshot, err := reconnectSessionRelease(ctx, root, id, bound.Ready.Release, io)
	var mismatch *bridge.RuntimeReleaseMismatch
	if errors.As(err, &mismatch) && mismatch.Observed == buildinfo.Version {
		// A prior upgrade may already have activated. Re-prove the same actor
		// against the ordinary current-release contract; never replay refresh.
		return reconnectSession(ctx, root, id, io)
	}
	return session, snapshot, err
}
