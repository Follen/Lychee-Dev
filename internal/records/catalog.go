// SPDX-License-Identifier: AGPL-3.0-or-later
// CASC metadata formats adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"
)

var (
	ErrMetadataFormat         = errors.New("records.invalid_metadata")
	ErrMetadataLimit          = errors.New("records.metadata_limit")
	ErrConfigurationIntegrity = errors.New("records.configuration_integrity")
	ErrBuildUnavailable       = errors.New("records.build_unavailable")
	ErrBuildAmbiguous         = errors.New("records.build_ambiguous")
)

// InstalledBuild preserves the installation's raw product identifier. Product
// slot names are not client identities; friendly-name mapping belongs to selection.
type InstalledBuild struct {
	Product     string `json:"product"`
	FullBuild   string `json:"fullBuild"`
	BuildConfig string `json:"buildConfig"`
	CDNConfig   string `json:"cdnConfig"`
	Active      bool   `json:"active"`
}

type ConfigDocument struct {
	Key    string              `json:"key"`
	SHA256 string              `json:"sha256"`
	Raw    []byte              `json:"-"`
	Fields map[string][]string `json:"fields"`
}

type BuildMetadata struct {
	Installed          InstalledBuild `json:"installed"`
	CatalogSHA256      string         `json:"catalogSHA256"`
	RootContentKey     string         `json:"rootContentKey"`
	EncodingContentKey string         `json:"encodingContentKey"`
	EncodingKey        string         `json:"encodingKey"`
	ContentBytes       int64          `json:"contentBytes"`
	EncodingBytes      int64          `json:"encodingBytes"`
	BuildDocument      ConfigDocument `json:"buildDocument"`
	CDNDocument        ConfigDocument `json:"cdnDocument"`
}

// ReadInstallCatalog reads only the explicitly named installation. Inactive and
// unfamiliar product rows remain visible; folder names never override metadata.
func ReadInstallCatalog(ctx context.Context, root string) ([]InstalledBuild, error) {
	dir, err := os.OpenRoot(root)
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	raw, err := readMetadata(ctx, dir, ".build.info", 1<<20)
	if err != nil {
		return nil, err
	}
	return installedRows(raw)
}

// ResolveLocalBuild fixes one active, exact product/build row and authenticates
// both configuration byte strings against their content-addressed MD5 keys.
// Region, locale and DBD commit are not inferred: callers still need them before
// constructing a DataPin. Missing local configuration does not trigger a network
// fallback or silently drop CDN identity. Returned documents own their bytes.
func ResolveLocalBuild(ctx context.Context, root, product, fullBuild string) (BuildMetadata, error) {
	var result BuildMetadata
	if !metadataProduct(product) || !metadataBuild(fullBuild) {
		return result, ErrMetadataFormat
	}
	dir, err := os.OpenRoot(root)
	if err != nil {
		return result, err
	}
	defer dir.Close()
	raw, err := readMetadata(ctx, dir, ".build.info", 1<<20)
	if err != nil {
		return result, err
	}
	rows, err := installedRows(raw)
	if err != nil {
		return result, err
	}
	found := false
	for _, row := range rows {
		if row.Product != product || row.FullBuild != fullBuild || !row.Active {
			continue
		}
		if found {
			return BuildMetadata{}, ErrBuildAmbiguous
		}
		result.Installed = row
		found = true
	}
	if !found {
		return result, ErrBuildUnavailable
	}
	sum := sha256.Sum256(raw)
	result.CatalogSHA256 = hex.EncodeToString(sum[:])
	result.BuildDocument, err = readConfiguration(ctx, dir, result.Installed.BuildConfig)
	if err != nil {
		return BuildMetadata{}, fmt.Errorf("build config: %w", err)
	}
	result.CDNDocument, err = readConfiguration(ctx, dir, result.Installed.CDNConfig)
	if err != nil {
		return BuildMetadata{}, fmt.Errorf("CDN config: %w", err)
	}
	return completeBuildMetadata(result)
}

// Both installation and CDN configurations carry the same content identities.
func completeBuildMetadata(result BuildMetadata) (BuildMetadata, error) {
	var err error
	fields := result.BuildDocument.Fields
	rootKeys, encodingKeys, sizes := fields["root"], fields["encoding"], fields["encoding-size"]
	if len(rootKeys) != 1 || !metadataKey(rootKeys[0]) || len(encodingKeys) != 2 || !metadataKey(encodingKeys[0]) || !metadataKey(encodingKeys[1]) || len(sizes) != 2 {
		return BuildMetadata{}, ErrMetadataFormat
	}
	if uid, exists := fields["build-uid"]; exists && (len(uid) != 1 || uid[0] != result.Installed.Product) {
		return BuildMetadata{}, fmt.Errorf("%w: product/config mismatch", ErrMetadataFormat)
	}
	result.RootContentKey, result.EncodingContentKey, result.EncodingKey = rootKeys[0], encodingKeys[0], encodingKeys[1]
	result.ContentBytes, err = positiveSize(sizes[0])
	if err != nil {
		return BuildMetadata{}, err
	}
	result.EncodingBytes, err = positiveSize(sizes[1])
	if err != nil {
		return BuildMetadata{}, err
	}
	return result, nil
}

func installedRows(raw []byte) ([]InstalledBuild, error) {
	lines := strings.Split(strings.TrimPrefix(string(raw), "\ufeff"), "\n")
	if len(lines) < 2 {
		return nil, ErrMetadataFormat
	}
	headers := strings.Split(strings.TrimSuffix(lines[0], "\r"), "|")
	if len(headers) > 128 {
		return nil, ErrMetadataLimit
	}
	columns := make(map[string]int, len(headers))
	for i, header := range headers {
		name, _, _ := strings.Cut(header, "!")
		name = strings.TrimSpace(name)
		if _, exists := columns[name]; exists || name == "" {
			return nil, ErrMetadataFormat
		}
		columns[name] = i
	}
	for _, required := range []string{"Product", "Version", "Build Key", "CDN Key", "Active"} {
		if _, ok := columns[required]; !ok {
			return nil, ErrMetadataFormat
		}
	}
	rows := make([]InstalledBuild, 0)
	for _, line := range lines[1:] {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		if len(rows) == 4096 {
			return nil, ErrMetadataLimit
		}
		values := strings.Split(line, "|")
		if len(values) != len(headers) {
			return nil, ErrMetadataFormat
		}
		get := func(key string) string { return strings.TrimSpace(values[columns[key]]) }
		row := InstalledBuild{Product: get("Product"), FullBuild: get("Version"), BuildConfig: get("Build Key"), CDNConfig: get("CDN Key"), Active: get("Active") == "1"}
		if !metadataProduct(row.Product) || !metadataBuild(row.FullBuild) || !metadataKey(row.BuildConfig) || !metadataKey(row.CDNConfig) || (get("Active") != "0" && get("Active") != "1") {
			return nil, ErrMetadataFormat
		}
		rows = append(rows, row)
	}
	return rows, nil
}

func readConfiguration(ctx context.Context, dir *os.Root, key string) (ConfigDocument, error) {
	var result ConfigDocument
	if !metadataKey(key) {
		return result, ErrMetadataFormat
	}
	raw, err := readMetadata(ctx, dir, filepath.Join("Data", "config", key[:2], key[2:4], key), 4<<20)
	if err != nil {
		return result, err
	}
	return parseConfiguration(raw, key)
}

func parseConfiguration(raw []byte, key string) (ConfigDocument, error) {
	var result ConfigDocument
	if len(raw) > 4<<20 {
		return result, ErrMetadataLimit
	}
	if !metadataKey(key) || !utf8.Valid(raw) || bytes.IndexByte(raw, 0) >= 0 {
		return result, ErrMetadataFormat
	}
	md := md5.Sum(raw)
	if hex.EncodeToString(md[:]) != key {
		return result, ErrConfigurationIntegrity
	}
	fields := make(map[string][]string)
	for _, line := range strings.Split(string(raw), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		name, value, ok := strings.Cut(line, "=")
		name = strings.TrimSpace(name)
		if !ok || !metadataField(name) {
			return result, ErrMetadataFormat
		}
		if _, exists := fields[name]; exists {
			return result, fmt.Errorf("%w: duplicate configuration field", ErrMetadataFormat)
		}
		if len(fields) == 4096 {
			return result, ErrMetadataLimit
		}
		fields[name] = strings.Fields(value)
	}
	sum := sha256.Sum256(raw)
	return ConfigDocument{Key: key, SHA256: hex.EncodeToString(sum[:]), Raw: raw, Fields: fields}, nil
}

func readMetadata(ctx context.Context, dir *os.Root, name string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	f, err := dir.Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, ErrMetadataFormat
	}
	if info.Size() > limit {
		return nil, ErrMetadataLimit
	}
	raw, err := io.ReadAll(io.LimitReader(&metadataReader{ctx, f}, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(raw)) > limit {
		return nil, ErrMetadataLimit
	}
	if !utf8.Valid(raw) || bytes.IndexByte(raw, 0) >= 0 {
		return nil, ErrMetadataFormat
	}
	return raw, nil
}

type metadataReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *metadataReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.source.Read(p)
}
func metadataKey(value string) bool {
	if len(value) != 32 {
		return false
	}
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
func metadataProduct(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_') {
			return false
		}
	}
	return true
}
func metadataField(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return true
}
func metadataBuild(value string) bool {
	parts := strings.Split(value, ".")
	if len(parts) != 4 {
		return false
	}
	for _, p := range parts {
		if p == "" {
			return false
		}
		for _, c := range p {
			if c < '0' || c > '9' {
				return false
			}
		}
		if _, err := strconv.ParseUint(p, 10, 32); err != nil {
			return false
		}
	}
	return true
}
func positiveSize(value string) (int64, error) {
	for _, c := range value {
		if c < '0' || c > '9' {
			return 0, ErrMetadataFormat
		}
	}
	n, err := strconv.ParseInt(value, 10, 64)
	if err != nil || n <= 0 || n > 1<<50 {
		return 0, ErrMetadataFormat
	}
	return n, nil
}
