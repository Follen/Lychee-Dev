package selection

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

// ErrListingOffline refuses the availability listing under --offline. This
// listing is inherently a bounded network use case; it never silently serves a
// remembered observation as if it were current.
var ErrListingOffline = errors.New("selection.listing_requires_network")

// ErrManifestFormat rejects malformed versions manifests.
var ErrManifestFormat = errors.New("selection.manifest_format")
var ErrManifestLimit = errors.New("selection.manifest_limit")

const (
	manifestMaxBytes   = 1 << 20
	manifestMaxRows    = 4096
	manifestMaxColumns = 128
)

// ReleaseRow is one versions-manifest release observation for one region.
type ReleaseRow struct {
	Region      string `json:"region"`
	FullBuild   string `json:"fullBuild"`
	BuildID     string `json:"buildId,omitempty"`
	BuildConfig string `json:"buildConfig"`
	CDNConfig   string `json:"cdnConfig"`
	Current     bool   `json:"current"`
}

// ProductAvailability lists the releases one product serves for one region,
// newest first. It replaces the old "casc products" capability.
type ProductAvailability struct {
	Product     string       `json:"product"`
	ProductCode string       `json:"productCode"`
	Locator     string       `json:"locator"`
	Rows        []ReleaseRow `json:"rows"`
	Rejected    int          `json:"rejected"`
	Error       string       `json:"error,omitempty"`
}

// Availability is one bounded remote listing over the supported products.
type Availability struct {
	Region     string                `json:"region"`
	ObservedAt time.Time             `json:"observedAt"`
	Products   []ProductAvailability `json:"products"`
}

// ManifestFetcher returns the raw versions manifest for one product code.
// Tests inject fixtures; production uses HTTPManifestFetcher.
type ManifestFetcher func(ctx context.Context, region, productCode string) ([]byte, error)

// AvailabilityOptions bounds one listing.
type AvailabilityOptions struct {
	Region string
	// Offline refuses the listing (ErrListingOffline); there is no stale
	// fallback masquerading as a current observation.
	Offline bool
	Fetch   ManifestFetcher
	// MaxRows bounds the rows kept per product (default 64, hard cap 4096).
	MaxRows int
}

// ListRemoteAvailability lists the available products and builds from the
// versions manifests for one region. Network use is bounded: at most one
// manifest per supported product, each capped at 1 MiB and 4096 rows. A single
// failing product is reported inline instead of failing the whole listing.
func ListRemoteAvailability(ctx context.Context, options AvailabilityOptions) (Availability, error) {
	result := Availability{Region: options.Region, ObservedAt: time.Now().UTC()}
	if options.Offline {
		return result, ErrListingOffline
	}
	if _, err := DataLocale(options.Region, "enUS"); err != nil {
		return result, err
	}
	fetch := options.Fetch
	if fetch == nil {
		fetch = HTTPManifestFetcher(nil)
	}
	maxRows := options.MaxRows
	if maxRows <= 0 {
		maxRows = 64
	}
	if maxRows > manifestMaxRows {
		maxRows = manifestMaxRows
	}
	baselines := VerifiedClientBaselines()
	result.Products = make([]ProductAvailability, 0, len(baselines))
	for _, baseline := range baselines {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		slot, err := DataProductSlot(baseline.Product)
		if err != nil {
			return Availability{}, err
		}
		locator := VersionsLocator(options.Region, slot)
		entry := ProductAvailability{Product: baseline.Product, ProductCode: baseline.ProductCode, Locator: locator}
		raw, err := fetch(ctx, options.Region, baseline.ProductCode)
		if err != nil {
			entry.Error = err.Error()
			result.Products = append(result.Products, entry)
			continue
		}
		rows, rejected, err := ParseVersionsManifest(raw, options.Region)
		if err != nil {
			entry.Error = err.Error()
			result.Products = append(result.Products, entry)
			continue
		}
		entry.Rejected = rejected
		if len(rows) > maxRows {
			rows = rows[:maxRows]
		}
		for i := range rows {
			rows[i].Current = i == 0
		}
		entry.Rows = rows
		result.Products = append(result.Products, entry)
	}
	return result, nil
}

// SelectRelease applies the historical-build resolution rule to manifest rows:
// without an exact build the newest row is selected; with an exact build only
// an exact match is acceptable. The CDN-side "still served" check happens
// during preparation and maps to the same error.
func SelectRelease(rows []ReleaseRow, exactBuild string) (ReleaseRow, error) {
	if exactBuild == "" {
		if len(rows) == 0 {
			return ReleaseRow{}, fmt.Errorf("%w: no release row for region", ErrTargetBuildUnavailable)
		}
		return rows[0], nil
	}
	if !build(exactBuild) {
		return ReleaseRow{}, fmt.Errorf("%w: malformed build %q", ErrTargetFormat, exactBuild)
	}
	for _, row := range rows {
		if row.FullBuild == exactBuild {
			return row, nil
		}
	}
	return ReleaseRow{}, fmt.Errorf("%w: %s", ErrTargetBuildUnavailable, exactBuild)
}

// VersionsLocator is the bounded manifest endpoint for one region and product.
func VersionsLocator(region, productCode string) string {
	if region == "cn" {
		return "https://cn.version.battlenet.com.cn/" + productCode + "/versions"
	}
	return "https://" + region + ".version.battle.net/" + productCode + "/versions"
}

// HTTPManifestFetcher fetches versions manifests with a bounded client:
// redirects are refused, responses are capped at 1 MiB and no other endpoint
// is ever contacted.
func HTTPManifestFetcher(client *http.Client) ManifestFetcher {
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Second}
	}
	return func(ctx context.Context, region, productCode string) ([]byte, error) {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, VersionsLocator(region, productCode), nil)
		if err != nil {
			return nil, err
		}
		copy := *client
		copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
		response, err := copy.Do(request)
		if err != nil {
			return nil, fmt.Errorf("selection.listing_http: %w", err)
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("selection.listing_http: status %d", response.StatusCode)
		}
		if response.ContentLength > manifestMaxBytes {
			return nil, ErrManifestLimit
		}
		raw, err := io.ReadAll(io.LimitReader(&manifestReader{ctx, response.Body}, manifestMaxBytes+1))
		if err != nil {
			return nil, fmt.Errorf("selection.listing_http: %w", err)
		}
		if int64(len(raw)) > manifestMaxBytes {
			return nil, ErrManifestLimit
		}
		return raw, nil
	}
}

// ParseVersionsManifest parses a BPSV versions manifest and keeps the rows
// whose Region cell lists the requested region (single or comma separated).
// Structurally invalid rows are rejected and counted instead of guessed.
func ParseVersionsManifest(raw []byte, region string) ([]ReleaseRow, int, error) {
	table, rejected, err := parseManifestTable(raw)
	if err != nil {
		return nil, 0, err
	}
	regionColumn := table.columns["Region"]
	rows := make([]ReleaseRow, 0, 8)
	for _, values := range table.rows {
		if !regionListed(values[regionColumn], region) {
			continue
		}
		row := ReleaseRow{
			Region:      region,
			FullBuild:   values[table.columns["VersionsName"]],
			BuildConfig: values[table.columns["BuildConfig"]],
			CDNConfig:   values[table.columns["CDNConfig"]],
		}
		if column, ok := table.columns["BuildId"]; ok {
			row.BuildID = values[column]
		}
		if !build(row.FullBuild) || !hexKey(row.BuildConfig) || !hexKey(row.CDNConfig) {
			rejected++
			continue
		}
		if row.BuildID != "" && !buildIDMatches(row.FullBuild, row.BuildID) {
			rejected++
			continue
		}
		rows = append(rows, row)
	}
	sort.SliceStable(rows, func(i, j int) bool { return buildNewer(rows[i].FullBuild, rows[j].FullBuild) })
	return rows, rejected, nil
}

// buildNewer orders full builds numerically per component so "newest first"
// never depends on lexicographic accidents.
func buildNewer(a, b string) bool {
	left, right := strings.Split(a, "."), strings.Split(b, ".")
	for i := 0; i < len(left) && i < len(right); i++ {
		la, _ := strconv.ParseUint(left[i], 10, 32)
		ra, _ := strconv.ParseUint(right[i], 10, 32)
		if la != ra {
			return la > ra
		}
	}
	return a > b
}

type manifestTable struct {
	columns map[string]int
	rows    [][]string
}

func parseManifestTable(raw []byte) (manifestTable, int, error) {
	var table manifestTable
	if len(raw) > manifestMaxBytes {
		return table, 0, ErrManifestLimit
	}
	if !utf8.Valid(raw) || strings.ContainsRune(string(raw), 0) {
		return table, 0, ErrManifestFormat
	}
	text := strings.TrimPrefix(string(raw), "\ufeff")
	lines := strings.Split(text, "\n")
	var header []string
	rejected := 0
	for _, line := range lines {
		line = strings.TrimSuffix(line, "\r")
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}
		if header == nil {
			parts := strings.Split(line, "|")
			if len(parts) > manifestMaxColumns {
				return table, 0, ErrManifestLimit
			}
			table.columns = make(map[string]int, len(parts))
			header = make([]string, 0, len(parts))
			for i, part := range parts {
				name, _, ok := strings.Cut(strings.TrimSpace(part), "!")
				name = strings.TrimSpace(name)
				if !ok || name == "" {
					return table, 0, ErrManifestFormat
				}
				if _, exists := table.columns[name]; exists {
					return table, 0, ErrManifestFormat
				}
				table.columns[name] = i
				header = append(header, name)
			}
			for _, required := range []string{"Region", "VersionsName", "BuildConfig", "CDNConfig"} {
				if _, ok := table.columns[required]; !ok {
					return table, 0, ErrManifestFormat
				}
			}
			continue
		}
		if len(table.rows) == manifestMaxRows {
			return table, 0, ErrManifestLimit
		}
		values := strings.Split(line, "|")
		if len(values) != len(header) {
			rejected++
			continue
		}
		for i := range values {
			values[i] = strings.TrimSpace(values[i])
		}
		table.rows = append(table.rows, values)
	}
	if header == nil {
		return table, 0, ErrManifestFormat
	}
	return table, rejected, nil
}

func regionListed(cell, region string) bool {
	for _, listed := range strings.Split(cell, ",") {
		if strings.TrimSpace(listed) == region {
			return true
		}
	}
	return false
}

func hexKey(value string) bool {
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

// buildIDMatches mirrors the manifest consistency rule: the decimal BuildId
// must agree with the last component of the full build.
func buildIDMatches(fullBuild, buildID string) bool {
	parts := strings.Split(fullBuild, ".")
	if len(parts) == 0 || buildID == "" {
		return false
	}
	want, err := strconv.ParseUint(parts[len(parts)-1], 10, 32)
	if err != nil {
		return false
	}
	got, err := strconv.ParseUint(buildID, 10, 32)
	return err == nil && got == want
}

type manifestReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *manifestReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.source.Read(p)
}
