package delivery

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"path/filepath"
)

func InstallSkill(ctx context.Context, release, destination, version string) (InstallationReceipt, error) {
	if filepath.Base(filepath.Clean(destination)) != "lycheedev" {
		return InstallationReceipt{}, fmt.Errorf("%w: skill directory must be named lycheedev", ErrInstallation)
	}
	return InstallFresh(ctx, release, destination, "skill", version)
}

type AddonDeployment struct {
	Client       selection.ClientInstallation `json:"client"`
	Installation InstallAssessment            `json:"installation"`
	Slots        *SlotAssessment              `json:"slots,omitempty"`
}

type SlotAssessment struct {
	State   string `json:"state"`
	Count   int    `json:"count"`
	Pending int    `json:"pending"`
	Detail  string `json:"detail,omitempty"`
}

func InspectAddonDeployment(ctx context.Context, clientDirectory string) (AddonDeployment, error) {
	client, err := records.InspectClientInstallation(ctx, clientDirectory)
	if err != nil {
		return AddonDeployment{}, err
	}
	status, err := InspectInstallation(ctx, AddonDirectory(client.Directory), "addon")
	if err != nil {
		return AddonDeployment{}, err
	}
	deployment := AddonDeployment{Client: client, Installation: status}
	if status.Receipt != nil && receiptUsesSlots(*status.Receipt) {
		pool, slotErr := InspectSlots(ctx, filepath.Join(client.Directory, "Interface", "AddOns"), status.Receipt.Version)
		deployment.Slots = &SlotAssessment{State: "managed", Count: len(pool.Files)}
		if slotErr != nil {
			deployment.Slots.State = "incomplete"
			deployment.Slots.Detail = slotErr.Error()
		} else {
			for _, slot := range pool.Files {
				if slot.PendingHash != "" {
					deployment.Slots.Pending++
				}
			}
			if deployment.Slots.Pending > 0 {
				deployment.Slots.State = "pending"
			}
		}
	}
	return deployment, nil
}

func InspectDeployment(ctx context.Context, destination, component string) (InstallAssessment, error) {
	return InspectInstallation(ctx, destination, component)
}

func RemoveSkill(ctx context.Context, destination, archive string) (Removal, error) {
	if filepath.Base(filepath.Clean(destination)) != "lycheedev" {
		return Removal{}, fmt.Errorf("%w: skill directory must be named lycheedev", ErrInstallation)
	}
	return RemoveInstallation(ctx, destination, archive, "skill")
}

func UpgradeSkill(ctx context.Context, release, destination, archive, version string, resume bool) (Upgrade, error) {
	if filepath.Base(filepath.Clean(destination)) != "lycheedev" {
		return Upgrade{}, fmt.Errorf("%w: skill directory must be named lycheedev", ErrInstallation)
	}
	if resume {
		return ResumeUpgrade(ctx, destination, archive, "skill")
	}
	return UpgradeInstallation(ctx, release, destination, archive, "skill", version)
}
