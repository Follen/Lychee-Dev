package selection

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"unicode/utf8"
)

var ErrClientIdentity = errors.New("selection.client_identity")

// ClientBuild is an active build-catalog observation supplied by the caller.
// It is a fallback for absent version.txt, not a folder identity override.
type ClientBuild struct{ ProductCode, FullBuild string }
type ClientInstallation struct {
	Directory   string `json:"directory"`
	Product     string `json:"product"`
	ProductCode string `json:"productCode"`
	// CatalogProduct preserves the observed reusable launcher slot independently
	// of the canonical product, so regional installations cannot borrow rows.
	CatalogProduct string `json:"catalogProduct,omitempty"`
	FullBuild      string `json:"fullBuild"`
	Interface      int    `json:"interface"`
	TOC            string `json:"toc"`
	IdentitySource string `json:"identitySource"`
}

var flavorWord = regexp.MustCompile(`\bwow[a-z0-9_]*\b`)

// InspectClient resolves an explicitly selected client from flavor and build
// catalog evidence. A directory locates a client; it never upgrades a retired
// beta product into the formal Forever product.
func InspectClient(ctx context.Context, directory string, active []ClientBuild) (ClientInstallation, error) {
	var result ClientInstallation
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return result, err
	}
	absolute, err = filepath.EvalSymlinks(absolute)
	if err != nil {
		return result, err
	}
	root, err := os.OpenRoot(absolute)
	if err != nil {
		return result, err
	}
	defer root.Close()
	product, version, source, flavorName := "", "", "", ""
	for _, name := range []string{".flavor.info", "flavor.info"} {
		text, err := clientText(ctx, root, name)
		if err != nil {
			return result, err
		}
		if text == "" {
			continue
		}
		matches := flavorWord.FindAllString(strings.ToLower(text), -1)
		if len(matches) != 1 {
			return result, fmt.Errorf("%w: invalid %s", ErrClientIdentity, name)
		}
		product, source, flavorName = matches[0], name, name
		break
	}
	result.ProductCode = product // Preserve observed flavor even on identity failure.
	if product == "wow_forever" || product == "wow_classic_beta" || product == "wow_cn_beta" {
		return result, fmt.Errorf("%w: retired Forever product %q; select wowf", ErrClientIdentity, product)
	}
	version, err = clientText(ctx, root, "version.txt")
	if err != nil {
		return result, err
	}
	if version != "" && !build(version) {
		return result, fmt.Errorf("%w: invalid version.txt", ErrClientIdentity)
	}
	folder := strings.ToLower(filepath.Base(absolute))
	if flavorName == "" && (folder == "_classic_beta_" || folder == "_cn_beta_") {
		return result, fmt.Errorf("%w: retired slot requires explicit current flavor", ErrClientIdentity)
	}
	if product == "" && version != "" {
		for _, baseline := range clientIdentityBaselines() {
			if strings.HasPrefix(version, baseline.BuildSeries+".") {
				product, source = baseline.ProductCode, "version.txt"
				break
			}
		}
		if product == "" {
			return result, fmt.Errorf("%w: unsupported build", ErrClientIdentity)
		}
	}
	if product == "" {
		for _, profile := range DataProducts() {
			if profile.Folder == folder {
				for _, candidate := range active {
					if candidate.ProductCode == profile.ProductCode {
						product, source = profile.ProductCode, "build-catalog"
					}
				}
			}
		}
	}
	// A family version alone cannot distinguish the old beta from formal wowf.
	if product == "wowf" && flavorName == "" {
		matched := false
		for _, candidate := range active {
			if candidate.ProductCode == "wowf" && (version == "" || candidate.FullBuild == version) {
				matched = true
			}
		}
		if !matched {
			return result, fmt.Errorf("%w: Forever requires wowf flavor or catalog evidence", ErrClientIdentity)
		}
	}
	if version == "" {
		for _, candidate := range active {
			if candidate.ProductCode != product {
				continue
			}
			if version != "" && version != candidate.FullBuild {
				return result, fmt.Errorf("%w: ambiguous active builds", ErrClientIdentity)
			}
			version = candidate.FullBuild
		}
	}
	var selected *ClientBaseline
	for _, baseline := range clientIdentityBaselines() {
		if baseline.ProductCode == product {
			result.Product = baseline.Product
			if strings.HasPrefix(version, baseline.BuildSeries+".") {
				value := baseline
				selected = &value
				break
			}
		}
	}
	if result.ProductCode == "" {
		result.ProductCode = product
	}
	if result.Product == "" {
		return result, fmt.Errorf("%w: unsupported product %q", ErrClientIdentity, product)
	}
	if !build(version) || selected == nil {
		return result, fmt.Errorf("%w: missing or unsupported product/build pair", ErrClientIdentity)
	}
	for _, candidate := range active {
		if candidate.ProductCode == product && candidate.FullBuild != version {
			return result, fmt.Errorf("%w: version conflicts with active catalog", ErrClientIdentity)
		}
	}
	return ClientInstallation{Directory: absolute, Product: selected.Product, ProductCode: product, FullBuild: version, Interface: selected.Interface, TOC: selected.TOC, IdentitySource: source}, nil
}

func clientText(ctx context.Context, root *os.Root, name string) (string, error) {
	file, err := root.Open(name)
	if errors.Is(err, os.ErrNotExist) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return "", err
	}
	if !info.Mode().IsRegular() || info.Size() > 4096 {
		return "", ErrClientIdentity
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	raw, err := io.ReadAll(io.LimitReader(file, 4097))
	if err != nil {
		return "", err
	}
	if len(raw) > 4096 || !utf8.Valid(raw) {
		return "", ErrClientIdentity
	}
	return strings.TrimSpace(strings.TrimPrefix(string(raw), "\ufeff")), nil
}
