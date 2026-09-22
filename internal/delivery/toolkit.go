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
}

func InspectAddonDeployment(ctx context.Context, clientDirectory string) (AddonDeployment, error) {
	client, err := records.InspectClientInstallation(ctx, clientDirectory)
	if err != nil {
		return AddonDeployment{}, err
	}
	status, err := InspectInstallation(ctx, filepath.Join(client.Directory, "Interface", "AddOns", "Lychee Dev"), "addon")
	if err != nil {
		return AddonDeployment{}, err
	}
	return AddonDeployment{Client: client, Installation: status}, nil
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
