package records

import (
	"context"
	"errors"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/selection"
)

// InspectClientInstallation joins client identity with active CASC metadata.
// Installation, live sessions and data preparation use the same observation.
func InspectClientInstallation(ctx context.Context, directory string) (selection.ClientInstallation, error) {
	return SelectClientInstallation(ctx, directory, "")
}

func inspectExactClient(ctx context.Context, directory string) (selection.ClientInstallation, error) {
	directory, err := filepath.Abs(directory)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	directory, err = filepath.EvalSymlinks(directory)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	rows, err := ReadInstallCatalog(ctx, filepath.Dir(directory))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return selection.ClientInstallation{}, err
	}
	var active []selection.ClientBuild
	for _, row := range rows {
		if row.Active {
			active = append(active, selection.ClientBuild{ProductCode: row.Product, FullBuild: row.FullBuild})
		}
	}
	client, err := selection.InspectClient(ctx, directory, active)
	if err != nil {
		return client, err
	}
	// version.txt cannot override contradictory active launcher metadata.
	slot, _ := selection.DataProductSlot(client.Product)
	if client.CatalogProduct != "" {
		slot = client.CatalogProduct
	}
	for _, row := range active {
		if (row.ProductCode == client.ProductCode || row.ProductCode == slot) && row.FullBuild != client.FullBuild {
			return client, ErrInstallationConflict
		}
	}
	return client, nil
}

// A data reader accepts either a CASC root or the selected client directory.
// Client metadata must agree with the pin before using its parent archives.
// Return the observed catalog product as well, preserving regional slot selection.
func dataInstallationRoot(ctx context.Context, directory, product, fullBuild string) (string, string, error) {
	if _, err := os.Stat(filepath.Join(directory, ".build.info")); err == nil {
		return directory, product, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", "", err
	}
	client, err := InspectClientInstallation(ctx, directory)
	if err != nil {
		if errors.Is(err, ErrInstallationConflict) {
			return "", "", errors.Join(ErrPinnedBuildChanged, err)
		}
		return "", "", err
	}
	if client.ProductCode != product || client.FullBuild != fullBuild {
		return "", "", ErrPinnedBuildChanged
	}
	if client.CatalogProduct != "" {
		product = client.CatalogProduct
	}
	return filepath.Dir(client.Directory), product, nil
}
