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
	Directory      string `json:"directory"`
	Product        string `json:"product"`
	ProductCode    string `json:"productCode"`
	FullBuild      string `json:"fullBuild"`
	Interface      int    `json:"interface"`
	TOC            string `json:"toc"`
	IdentitySource string `json:"identitySource"`
}

var flavorWord = regexp.MustCompile(`\bwow[a-z0-9_]*\b`)

// InspectClient resolves only one explicitly selected client. Product evidence
// outranks version evidence, then a known folder is the last fallback. A reused
// test slot is never treated as Forever without a matching build and identity.
// Unsupported or contradictory evidence fails; it does not select another client.
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
	product, version, source := "", "", ""
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
		product, source = matches[0], name
		break
	}
	version, err = clientText(ctx, root, "version.txt")
	if err != nil {
		return result, err
	}
	if version != "" && !build(version) {
		return result, fmt.Errorf("%w: invalid version.txt", ErrClientIdentity)
	}
	folder := strings.ToLower(filepath.Base(absolute))
	if product == "" && version != "" {
		for _, baseline := range VerifiedClientBaselines() {
			if strings.HasPrefix(version, baseline.BuildSeries+".") {
				product, source = baseline.ProductCode, "version.txt"
				break
			}
		}
		// A MoP build in the historical beta slot is not a supported install.
		if folder == "_classic_beta_" && !strings.HasPrefix(version, "1.60.1.") {
			return result, fmt.Errorf("%w: unsupported test track", ErrClientIdentity)
		}
		if product == "" {
			return result, fmt.Errorf("%w: unsupported build", ErrClientIdentity)
		}
	}
	if product == "" {
		product = map[string]string{"_retail_": "wow", "_classic_": "wow_classic", "_classic_titan_": "wow_classic_titan", "_forever_": "wow_forever"}[folder]
		source = "folder"
	}
	var selected *ClientBaseline
	for _, baseline := range VerifiedClientBaselines() {
		if baseline.ProductCode == product {
			value := baseline
			selected = &value
			break
		}
	}
	if selected == nil {
		return result, fmt.Errorf("%w: unsupported product %q", ErrClientIdentity, product)
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
	if !build(version) || !strings.HasPrefix(version, selected.BuildSeries+".") {
		return result, fmt.Errorf("%w: missing or unsupported product/build pair", ErrClientIdentity)
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
