package records

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

var (
	ErrInstallationMissing   = errors.New("selection.installation_missing")
	ErrInstallationAmbiguous = errors.New("selection.installation_ambiguous")
	ErrInstallationProduct   = errors.New("selection.installation_product_mismatch")
	ErrInstallationConflict  = errors.New("selection.installation_metadata_conflict")
)

type InstallationIssue struct {
	Directory   string `json:"directory"`
	ProductCode string `json:"productCode,omitempty"`
	Reason      string `json:"reason"`
}

type InstallationDiscovery struct {
	Directory  string                         `json:"directory"`
	Candidates []selection.ClientInstallation `json:"candidates"`
	Issues     []InstallationIssue            `json:"issues"`
}

type InstallationSelectionError struct {
	Kind      error
	Product   string
	Discovery InstallationDiscovery
}

func (e *InstallationSelectionError) Unwrap() error { return e.Kind }
func (e *InstallationSelectionError) Is(target error) bool {
	return target == e.Kind || target == selection.ErrClientIdentity && e.Kind == ErrInstallationConflict
}
func (e *InstallationSelectionError) Error() string {
	return fmt.Sprintf("%v: scope %q product %q; inspect candidates/issues and select a verified client directory", e.Kind, e.Discovery.Directory, e.Product)
}

// DiscoverClientInstallations inspects one explicit client or the immediate
// clients under a game root. Catalog rows alone never create installed clients.
// It does not traverse arbitrary trees, touch archives, or use the network.
func DiscoverClientInstallations(ctx context.Context, directory string) (InstallationDiscovery, error) {
	report := InstallationDiscovery{Candidates: []selection.ClientInstallation{}, Issues: []InstallationIssue{}}
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return report, err
	}
	report.Directory = absolute
	absolute, err = filepath.EvalSymlinks(absolute)
	if errors.Is(err, os.ErrNotExist) {
		return report, &InstallationSelectionError{Kind: ErrInstallationMissing, Discovery: report}
	}
	if err != nil {
		return report, err
	}
	report.Directory = absolute
	markers, err := clientMarkers(absolute)
	if err != nil {
		return report, err
	}
	_, catalogErr := os.Stat(filepath.Join(absolute, ".build.info"))
	if catalogErr != nil && !errors.Is(catalogErr, os.ErrNotExist) {
		return report, catalogErr
	}
	// Metadata makes an explicit client authoritative even when renamed. Known
	// client folders without local metadata can use their parent launcher row.
	exact := markers || catalogErr != nil && knownClientFolder(filepath.Base(absolute))
	paths := []string{absolute}
	if !exact {
		root, err := os.Open(absolute)
		if err != nil {
			return report, err
		}
		entries, readErr := root.ReadDir(257)
		closeErr := root.Close()
		if readErr != nil && !errors.Is(readErr, io.EOF) {
			return report, readErr
		}
		if closeErr != nil {
			return report, closeErr
		}
		if len(entries) > 256 {
			return report, ErrMetadataLimit
		}
		paths = nil
		for _, entry := range entries {
			if err := ctx.Err(); err != nil {
				return report, err
			}
			if !entry.IsDir() {
				continue
			}
			child := filepath.Join(absolute, entry.Name())
			present, err := clientMarkers(child)
			if err != nil {
				return report, err
			}
			if !present && knownClientFolder(entry.Name()) {
				for _, name := range []string{"Wow.exe", "WowClassic.exe", "WowB.exe"} {
					info, err := os.Stat(filepath.Join(child, name))
					if err == nil && info.Mode().IsRegular() {
						present = true
					}
					if err != nil && !errors.Is(err, os.ErrNotExist) {
						return report, err
					}
				}
			}
			if present {
				paths = append(paths, child)
			}
		}
		sort.Strings(paths)
	}
	for _, path := range paths {
		if err := ctx.Err(); err != nil {
			return report, err
		}
		client, err := inspectExactClient(ctx, path)
		if err != nil {
			if !errors.Is(err, selection.ErrClientIdentity) && !errors.Is(err, ErrInstallationConflict) {
				return report, err
			}
			report.Issues = append(report.Issues, InstallationIssue{Directory: path, ProductCode: client.ProductCode, Reason: err.Error()})
			continue
		}
		if !exact && !strings.EqualFold(filepath.Dir(client.Directory), absolute) {
			report.Issues = append(report.Issues, InstallationIssue{Directory: path, Reason: "client resolves outside selected game root"})
			continue
		}
		report.Candidates = append(report.Candidates, client)
	}
	return report, nil
}

func SelectClientInstallation(ctx context.Context, directory, product string) (selection.ClientInstallation, error) {
	if product != "" {
		if _, err := selection.DataProduct(product); err != nil {
			return selection.ClientInstallation{}, err
		}
	}
	report, err := DiscoverClientInstallations(ctx, directory)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	kind := ErrInstallationMissing
	if len(report.Candidates) > 0 && product != "" {
		kind = ErrInstallationProduct
	}
	var matches []selection.ClientInstallation
	for _, client := range report.Candidates {
		if product == "" || client.Product == product {
			matches = append(matches, client)
		}
	}
	// A known foreign flavor can be excluded by an explicit product constraint.
	// Unknown identity or a conflicting requested product still blocks selection.
	productCode, _ := selection.DataProduct(product)
	slot, _ := selection.DataProductSlot(product)
	conflict := false
	for _, issue := range report.Issues {
		if product == "" || issue.ProductCode == "" || issue.ProductCode == productCode || issue.ProductCode == slot {
			conflict = true
		}
	}
	if len(matches) > 1 {
		kind = ErrInstallationAmbiguous
	} else if conflict {
		kind = ErrInstallationConflict
	} else if len(matches) == 1 {
		return matches[0], nil
	}
	return selection.ClientInstallation{}, &InstallationSelectionError{Kind: kind, Product: product, Discovery: report}
}

func clientMarkers(directory string) (bool, error) {
	for _, name := range []string{".flavor.info", "flavor.info", "version.txt"} {
		if _, err := os.Stat(filepath.Join(directory, name)); err == nil {
			return true, nil
		} else if !errors.Is(err, os.ErrNotExist) {
			return false, err
		}
	}
	return false, nil
}

func knownClientFolder(name string) bool {
	switch strings.ToLower(name) {
	case "_retail_", "_classic_", "_classic_titan_", "_classic_beta_", "_forever_":
		return true
	}
	return false
}
