// SPDX-License-Identifier: AGPL-3.0-or-later
package records

import (
	"bytes"
	"net"
	"strconv"
	"strings"
	"unicode/utf8"
)

const (
	remoteCatalogMaxBytes   = 1 << 20
	remoteCatalogMaxColumns = 128
	remoteCatalogMaxRows    = 4096
)

type remoteRelease struct {
	FullBuild   string `json:"fullBuild"`
	BuildConfig string `json:"buildConfig"`
	CDNConfig   string `json:"cdnConfig"`
}

type distributionRoute struct {
	Path  string   `json:"path"`
	Hosts []string `json:"hosts"`
}

type remoteBPSV struct {
	columns map[string]int
	rows    [][]string
}

// parseReleaseCatalog selects one release row from a versions manifest for the
// requested region and validates only that row. Historical remote builds
// resolve only through an exact full build (decision 13): the exact build
// constrains the region's rows first, and without one the region's current row
// is the release. The selected row's CDN configurations are still required to
// be served; that check happens downstream and fails unavailability-style.
func parseReleaseCatalog(raw []byte, region, expectedBuild string) (remoteRelease, error) {
	table, err := parseRemoteBPSV(raw, []string{"Region", "BuildConfig", "CDNConfig", "BuildId", "VersionsName"})
	if err != nil {
		return remoteRelease{}, err
	}

	regionColumn := table.columns["Region"]
	buildColumn := table.columns["VersionsName"]
	regionRows := make([][]string, 0, 1)
	for _, row := range table.rows {
		if region == "" || row[regionColumn] != region {
			continue
		}
		regionRows = append(regionRows, row)
	}
	if len(regionRows) == 0 {
		return remoteRelease{}, ErrBuildUnavailable
	}
	row, err := selectReleaseRow(regionRows, buildColumn, expectedBuild)
	if err != nil {
		return remoteRelease{}, err
	}

	fullBuild := row[buildColumn]
	buildConfig := row[table.columns["BuildConfig"]]
	cdnConfig := row[table.columns["CDNConfig"]]
	buildID := row[table.columns["BuildId"]]
	if !metadataBuild(fullBuild) || !metadataKey(buildConfig) || !metadataKey(cdnConfig) || !sameBuildID(fullBuild, buildID) {
		return remoteRelease{}, ErrMetadataFormat
	}
	return remoteRelease{FullBuild: fullBuild, BuildConfig: buildConfig, CDNConfig: cdnConfig}, nil
}

// selectReleaseRow applies the historical remote build rule (decision 13) to
// one region's manifest rows. An exact full build names one historical row and
// must match it exactly: no matching row is unavailable, while more than one
// row for the same exact build is genuine ambiguity. Without an exact build
// the region's current row is the release.
func selectReleaseRow(rows [][]string, buildColumn int, expectedBuild string) ([]string, error) {
	if expectedBuild == "" {
		return currentReleaseRow(rows, buildColumn)
	}
	var match []string
	for _, row := range rows {
		if row[buildColumn] != expectedBuild {
			continue
		}
		if match != nil {
			return nil, ErrBuildAmbiguous
		}
		match = row
	}
	if match == nil {
		return nil, ErrBuildUnavailable
	}
	return match, nil
}

// currentReleaseRow identifies the region's current manifest row: the unique
// newest VersionsName, compared numerically per component. A sole row is the
// region's only release candidate and keeps its own validation errors. Among
// several rows, a row without a well-formed build cannot claim currency, and
// equally new rows leave the current row unidentified.
func currentReleaseRow(rows [][]string, buildColumn int) ([]string, error) {
	if len(rows) == 1 {
		return rows[0], nil
	}
	var current []string
	tied := false
	for _, row := range rows {
		candidate := row[buildColumn]
		if !metadataBuild(candidate) {
			continue
		}
		switch {
		case current == nil || buildNewer(candidate, current[buildColumn]):
			current, tied = row, false
		case !buildNewer(current[buildColumn], candidate):
			tied = true
		}
	}
	if current == nil || tied {
		return nil, ErrBuildAmbiguous
	}
	return current, nil
}

// buildNewer orders full builds numerically per component so recency never
// depends on lexicographic accidents. Both arguments are valid metadataBuild
// values; equal builds are not newer.
func buildNewer(a, b string) bool {
	left, right := strings.Split(a, "."), strings.Split(b, ".")
	for i := range left {
		la, _ := strconv.ParseUint(left[i], 10, 32)
		ra, _ := strconv.ParseUint(right[i], 10, 32)
		if la != ra {
			return la > ra
		}
	}
	return false
}

func parseDistributionCatalog(raw []byte, region string) (distributionRoute, error) {
	table, err := parseRemoteBPSV(raw, []string{"Name", "Path", "Hosts"})
	if err != nil {
		return distributionRoute{}, err
	}

	nameColumn := table.columns["Name"]
	matches := make([][]string, 0, 1)
	for _, row := range table.rows {
		if region != "" && row[nameColumn] == region {
			matches = append(matches, row)
		}
	}
	if len(matches) == 0 {
		return distributionRoute{}, ErrBuildUnavailable
	}
	if len(matches) > 1 {
		return distributionRoute{}, ErrBuildAmbiguous
	}

	row := matches[0]
	path := row[table.columns["Path"]]
	hosts := strings.Fields(row[table.columns["Hosts"]])
	if !validDistributionPath(path) || len(hosts) == 0 || len(hosts) > 16 {
		return distributionRoute{}, ErrMetadataFormat
	}

	seen := make(map[string]struct{}, len(hosts))
	for _, host := range hosts {
		if !validDistributionHost(host) {
			return distributionRoute{}, ErrMetadataFormat
		}
		key := strings.ToLower(host)
		if _, exists := seen[key]; exists {
			return distributionRoute{}, ErrMetadataFormat
		}
		seen[key] = struct{}{}
	}
	return distributionRoute{Path: path, Hosts: hosts}, nil
}

func parseRemoteBPSV(raw []byte, required []string) (remoteBPSV, error) {
	if len(raw) > remoteCatalogMaxBytes {
		return remoteBPSV{}, ErrMetadataLimit
	}
	if !utf8.Valid(raw) || bytes.IndexByte(raw, 0) >= 0 {
		return remoteBPSV{}, ErrMetadataFormat
	}
	if bytes.HasPrefix(raw, []byte{0xef, 0xbb, 0xbf}) {
		raw = raw[3:]
	}

	lines := strings.Split(string(raw), "\n")
	var header []string
	var columns map[string]int
	rows := make([][]string, 0)
	for _, line := range lines {
		if strings.HasSuffix(line, "\r") {
			line = strings.TrimSuffix(line, "\r")
		}
		if strings.ContainsRune(line, '\r') {
			return remoteBPSV{}, ErrMetadataFormat
		}
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}
		if header == nil {
			var err error
			header, columns, err = parseRemoteHeader(line)
			if err != nil {
				return remoteBPSV{}, err
			}
			for _, name := range required {
				if _, ok := columns[name]; !ok {
					return remoteBPSV{}, ErrMetadataFormat
				}
			}
			continue
		}
		if len(rows) == remoteCatalogMaxRows {
			return remoteBPSV{}, ErrMetadataLimit
		}
		values := strings.Split(line, "|")
		if len(values) != len(header) {
			return remoteBPSV{}, ErrMetadataFormat
		}
		for i := range values {
			values[i] = strings.TrimSpace(values[i])
		}
		rows = append(rows, values)
	}
	if header == nil {
		return remoteBPSV{}, ErrMetadataFormat
	}
	return remoteBPSV{columns: columns, rows: rows}, nil
}

func parseRemoteHeader(line string) ([]string, map[string]int, error) {
	parts := strings.Split(line, "|")
	if len(parts) > remoteCatalogMaxColumns {
		return nil, nil, ErrMetadataLimit
	}
	if len(parts) == 0 {
		return nil, nil, ErrMetadataFormat
	}
	columns := make(map[string]int, len(parts))
	names := make([]string, len(parts))
	for i, part := range parts {
		name, typeSpec, ok := strings.Cut(strings.TrimSpace(part), "!")
		name = strings.TrimSpace(name)
		typeSpec = strings.TrimSpace(typeSpec)
		if !ok || name == "" || !validRemoteTypeSpec(typeSpec) {
			return nil, nil, ErrMetadataFormat
		}
		if _, exists := columns[name]; exists {
			return nil, nil, ErrMetadataFormat
		}
		columns[name] = i
		names[i] = name
	}
	return names, columns, nil
}

func validRemoteTypeSpec(value string) bool {
	typeName, width, ok := strings.Cut(value, ":")
	if !ok || typeName == "" || width == "" {
		return false
	}
	for _, c := range typeName {
		if !(c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return false
		}
	}
	for _, c := range width {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

func sameBuildID(fullBuild, buildID string) bool {
	parts := strings.Split(fullBuild, ".")
	if len(parts) == 0 || buildID == "" {
		return false
	}
	for _, c := range buildID {
		if c < '0' || c > '9' {
			return false
		}
	}
	want, err := strconv.ParseUint(parts[len(parts)-1], 10, 32)
	if err != nil {
		return false
	}
	got, err := strconv.ParseUint(buildID, 10, 32)
	return err == nil && got == want
}

func validDistributionPath(path string) bool {
	if path == "" || len(path) > 256 || strings.HasPrefix(path, "/") || strings.HasSuffix(path, "/") {
		return false
	}
	for _, segment := range strings.Split(path, "/") {
		if segment == "" || segment == "." || segment == ".." {
			return false
		}
		for _, c := range segment {
			if !(c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.') {
				return false
			}
		}
	}
	return true
}

func validDistributionHost(host string) bool {
	if host == "" || len(host) > 253 || net.ParseIP(host) != nil {
		return false
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 || allDecimalLabels(labels) {
		return false
	}
	for _, label := range labels {
		if label == "" || len(label) > 63 || !isASCIIAlphaNumeric(label[0]) || !isASCIIAlphaNumeric(label[len(label)-1]) {
			return false
		}
		for i := 1; i < len(label)-1; i++ {
			if !isASCIIAlphaNumeric(label[i]) && label[i] != '-' {
				return false
			}
		}
	}
	return true
}

func allDecimalLabels(labels []string) bool {
	for _, label := range labels {
		for _, c := range label {
			if c < '0' || c > '9' {
				return false
			}
		}
	}
	return true
}

func isASCIIAlphaNumeric(c byte) bool {
	return c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c >= '0' && c <= '9'
}
