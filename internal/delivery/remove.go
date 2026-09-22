package delivery

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

type Removal struct {
	State   string               `json:"state"`
	Archive string               `json:"archive,omitempty"`
	Receipt *InstallationReceipt `json:"receipt,omitempty"`
}

// RemoveInstallation moves an unchanged owned installation to an explicit
// recovery destination. Nothing is deleted. The archive must be outside the
// discovery parent (skills/ or AddOns/) and on the same filesystem; no copy/delete
// fallback is used. The caller may later restore or discard it explicitly.
func RemoveInstallation(ctx context.Context, target, archive, component string) (Removal, error) {
	if component != "addon" && component != "skill" {
		return Removal{}, ErrInstallation
	}
	parent, target, err := installationDestination(target)
	if err != nil {
		return Removal{}, err
	}
	_, archive, err = installationDestination(archive)
	if err != nil {
		return Removal{}, err
	}
	if err := outsideDiscovery(parent, archive); err != nil {
		return Removal{}, err
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(parent, ".lycheedev-locks"), "installation:"+strings.ToLower(target))
	if err != nil {
		return Removal{}, err
	}
	defer lease.Close()
	assessment, err := InspectInstallation(ctx, target, component)
	if err != nil {
		return Removal{}, err
	}
	if assessment.State == "absent" {
		return Removal{State: "absent"}, nil
	}
	if assessment.State != "managed" {
		return Removal{}, fmt.Errorf("%w: %s", ErrConflict, assessment.State)
	}
	if err := ctx.Err(); err != nil {
		return Removal{}, err
	}
	if err := publishDirectory(target, archive); err != nil {
		return Removal{}, err
	}
	return Removal{State: "archived", Archive: archive, Receipt: assessment.Receipt}, nil
}
