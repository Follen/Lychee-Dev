package records

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

// InspectClientInstallation joins client identity with active CASC metadata.
// Installation, live sessions and data preparation use the same observation.
func InspectClientInstallation(ctx context.Context, directory string) (selection.ClientInstallation, error) {
	return SelectClientInstallation(ctx, directory, "")
}

// InspectRetainedClientInstallation rechecks a connection's original retired
// identity. It is used only after opening that saved connection, never for new
// target discovery. Current installations still use the common admission path.
func InspectRetainedClientInstallation(ctx context.Context, expected selection.ClientInstallation) (selection.ClientInstallation, error) {
	if expected.ProductCode != "wow_forever" {
		return InspectClientInstallation(ctx, expected.Directory)
	}
	if expected.Product != "forever" || expected.Interface != 16001 || expected.TOC != selection.MainTOC || !strings.HasPrefix(expected.FullBuild, "1.60.1.") {
		return selection.ClientInstallation{}, ErrPinnedBuildChanged
	}
	directory, err := filepath.EvalSymlinks(expected.Directory)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	root, slot, err := historicalDataInstallationRoot(ctx, directory, expected.ProductCode, expected.FullBuild)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	if expected.CatalogProduct != "" && expected.CatalogProduct != slot {
		return selection.ClientInstallation{}, ErrPinnedBuildChanged
	}
	rows, err := ReadInstallCatalog(ctx, root)
	if errors.Is(err, os.ErrNotExist) {
		// A version file is the original pre-catalog observation, not a guessed build.
		raw, readErr := os.ReadFile(filepath.Join(directory, "version.txt"))
		if readErr != nil || strings.TrimSpace(strings.TrimPrefix(string(raw), "\ufeff")) != expected.FullBuild {
			return selection.ClientInstallation{}, ErrPinnedBuildChanged
		}
	} else if err != nil {
		return selection.ClientInstallation{}, err
	} else {
		matched := false
		for _, row := range rows {
			if row.Active && row.Product == slot {
				if row.FullBuild != expected.FullBuild || matched {
					return selection.ClientInstallation{}, ErrPinnedBuildChanged
				}
				matched = true
			}
		}
		if !matched {
			return selection.ClientInstallation{}, ErrPinnedBuildChanged
		}
	}
	observed := expected
	observed.Directory = directory
	if slot != "wow_forever" {
		observed.CatalogProduct = slot
	}
	return observed, nil
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
	if product == "wow_forever" || product == "wow_classic_beta" || product == "wow_cn_beta" {
		return historicalDataInstallationRoot(ctx, directory, product, fullBuild)
	}
	client, err := InspectClientInstallation(ctx, directory)
	if err != nil {
		if errors.Is(err, ErrInstallationConflict) {
			return "", "", errors.Join(ErrPinnedBuildChanged, err)
		}
		return "", "", err
	}
	if client.ProductCode != product && client.CatalogProduct != product || client.FullBuild != fullBuild {
		return "", "", ErrPinnedBuildChanged
	}
	if client.CatalogProduct != "" {
		product = client.CatalogProduct
	}
	return filepath.Dir(client.Directory), product, nil
}

// This path only consumes an existing pin. It never supplies installation,
// target or Live discovery with a retired product.
var historicalFlavorWord = regexp.MustCompile(`\bwow[a-z0-9_]*\b`)

func historicalDataInstallationRoot(ctx context.Context, directory, product, fullBuild string) (string, string, error) {
	root, err := os.OpenRoot(directory)
	if err != nil {
		return "", "", err
	}
	defer root.Close()
	var flavor string
	for _, name := range []string{".flavor.info", "flavor.info"} {
		raw, err := readMetadata(ctx, root, name, 4096)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return "", "", err
		}
		// Keep the accepted flavor syntax and ambiguity rule used by current
		// client inspection, while admitting only the retained product codes.
		matches := historicalFlavorWord.FindAllString(strings.ToLower(string(raw)), -1)
		if len(matches) != 1 {
			return "", "", ErrPinnedBuildChanged
		}
		flavor = matches[0]
		break
	}
	if flavor != "wow_forever" && flavor != "wow_classic_beta" && flavor != "wow_cn_beta" || flavor != product && product != "wow_forever" {
		return "", "", ErrPinnedBuildChanged
	}
	if raw, err := readMetadata(ctx, root, "version.txt", 4096); err == nil {
		if strings.TrimSpace(strings.TrimPrefix(string(raw), "\ufeff")) != fullBuild {
			return "", "", ErrPinnedBuildChanged
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", "", err
	}
	return filepath.Dir(root.Name()), flavor, nil
}
