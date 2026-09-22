// SPDX-License-Identifier: AGPL-3.0-or-later
// Hotfix provider semantics adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Shared request/result model for every hotfix provider: the local DBCache use
// case, the Wago page provider and DBCache snapshot providers. Filters always
// select physical hotfix records with their original status and payloads;
// static DB2 rows are never overlaid on these results (DAT-07).

const (
	RemoteHotfixResultSchema = "lycheedev.hotfix-result.v1"
	HotfixCursorSchema       = "lycheedev.hotfix-cursor.v1"
)

var (
	ErrHotfixQuery         = errors.New("records.hotfix_query")
	ErrHotfixFilter        = errors.New("records.hotfix_filter_unsupported")
	ErrHotfixBudget        = errors.New("records.hotfix_budget")
	ErrHotfixPageDrift     = errors.New("records.hotfix_page_drift")
	ErrHotfixCacheCorrupt  = errors.New("records.hotfix_cache_corrupt")
	ErrHotfixCacheMiss     = errors.New("records.hotfix_cache_miss")
	ErrHotfixCacheWrite    = errors.New("records.hotfix_cache_write")
	ErrHotfixOffline       = errors.New("records.hotfix_offline")
	ErrHotfixWagoProtocol  = errors.New("records.hotfix_wago_protocol")
	ErrHotfixWagoHTTP      = errors.New("records.hotfix_wago_http")
	ErrHotfixSnapshotStale = errors.New("records.hotfix_snapshot_stale")
	ErrHotfixCursor        = errors.New("records.hotfix_cursor")
)

// HotfixQuery carries the explicit product/build/region/locale context every
// provider requires plus the shared physical filters. Definition names a pinned
// DBD definition for decoded fields; without it results keep raw payloads only.
type HotfixQuery struct {
	Product    string             `json:"product"`
	FullBuild  string             `json:"fullBuild"`
	Region     string             `json:"region"`
	Locale     string             `json:"locale"`
	Table      string             `json:"table,omitempty"`
	TableHash  *uint32            `json:"tableHash,omitempty"`
	RecordID   *uint32            `json:"recordId,omitempty"`
	PushID     *int32             `json:"push,omitempty"`
	RegionID   *uint32            `json:"regionId,omitempty"`
	Status     *uint8             `json:"status,omitempty"`
	From       *time.Time         `json:"from,omitempty"`
	To         *time.Time         `json:"to,omitempty"`
	Search     string             `json:"search,omitempty"`
	Latest     bool               `json:"latest,omitempty"`
	Page       int                `json:"page,omitempty"`
	Limit      int                `json:"limit"`
	Cursor     string             `json:"cursor,omitempty"`
	Definition *schema.Definition `json:"-"`
}

func (q HotfixQuery) Validate() error {
	if _, err := selection.DataProduct(q.Product); err != nil {
		return fmt.Errorf("%w: %v", ErrHotfixQuery, err)
	}
	if _, err := selection.DataLocale(q.Region, q.Locale); err != nil {
		return fmt.Errorf("%w: %v", ErrHotfixQuery, err)
	}
	if _, err := hotfixBuildNumber(q.FullBuild); err != nil {
		return err
	}
	if q.Table != "" && q.TableHash != nil {
		return fmt.Errorf("%w: select table or table-hash, not both", ErrHotfixQuery)
	}
	if q.From != nil && q.To != nil && q.From.After(*q.To) {
		return fmt.Errorf("%w: from must not be after to", ErrHotfixQuery)
	}
	if q.Page < 0 {
		return fmt.Errorf("%w: page must be non-negative", ErrHotfixQuery)
	}
	if q.Limit < 1 || q.Limit > 200 {
		return fmt.Errorf("%w: limit must be 1..200", ErrHotfixQuery)
	}
	return nil
}

// HotfixStatusFilter validates a raw --status value in 0..255.
func HotfixStatusFilter(value int) (*uint8, error) {
	if value < 0 || value > 255 {
		return nil, fmt.Errorf("%w: status must be 0..255", ErrHotfixQuery)
	}
	status := uint8(value)
	return &status, nil
}

// ParseHotfixTime accepts RFC3339 or the source-native "2006-01-02 15:04:05"
// layout. Nothing else is guessed.
func ParseHotfixTime(value string) (time.Time, error) {
	if t, err := time.Parse(time.RFC3339, value); err == nil {
		return t, nil
	}
	if t, err := time.Parse("2006-01-02 15:04:05", value); err == nil {
		return t, nil
	}
	return time.Time{}, fmt.Errorf("%w: time must be RFC3339 or 2006-01-02 15:04:05", ErrHotfixQuery)
}

// HotfixUint32, HotfixInt32 and HotfixUint8 build explicit filter values.
func HotfixUint32(value uint32) *uint32 { return &value }
func HotfixInt32(value int32) *int32    { return &value }
func HotfixUint8(value uint8) *uint8    { return &value }

// hotfixBuildNumber extracts the DBCache build number from an exact four
// segment build such as 12.0.7.68974.
func hotfixBuildNumber(fullBuild string) (uint32, error) {
	if !metadataBuild(fullBuild) {
		return 0, fmt.Errorf("%w: build must be four numeric segments", ErrHotfixQuery)
	}
	parts := strings.Split(fullBuild, ".")
	number, err := strconv.ParseUint(parts[3], 10, 32)
	if err != nil {
		return 0, fmt.Errorf("%w: build %q has no cache build number", ErrHotfixQuery, fullBuild)
	}
	return uint32(number), nil
}

// hotfixBuildMatches keeps the legacy suffix rule: an exact four segment build
// matches records carrying only the trailing build number.
func hotfixBuildMatches(query, record string) bool {
	if record == "" || query == "" {
		return false
	}
	if query == record {
		return true
	}
	return strings.HasSuffix(query, "."+record) || strings.HasSuffix(record, "."+query)
}

// HotfixRecord is one physical hotfix record from exactly one provider. Raw
// payloads are preserved (payloadHex for DBCache bytes, payload for the JSON
// column payload of remote rows); Fields and DecodeState appear only when the
// caller supplied a pinned DBD definition. decodeState is one of decoded,
// no_payload and not_valid.
type HotfixRecord struct {
	Provider      string          `json:"provider"`
	ID            uint64          `json:"id,omitempty"`
	Push          int32           `json:"push"`
	UniqueID      *uint32         `json:"uniqueId,omitempty"`
	RegionID      *uint32         `json:"regionId,omitempty"`
	TableHash     *uint32         `json:"tableHash,omitempty"`
	TableName     string          `json:"tableName,omitempty"`
	RecordID      uint32          `json:"recordId"`
	Status        uint8           `json:"status"`
	Build         string          `json:"build"`
	Locale        string          `json:"locale,omitempty"`
	CreatedAt     string          `json:"createdAt,omitempty"`
	Index         *uint32         `json:"index,omitempty"`
	PayloadLength int             `json:"payloadLength"`
	PayloadHex    string          `json:"payloadHex,omitempty"`
	Payload       json.RawMessage `json:"payload,omitempty"`
	Fields        map[string]any  `json:"fields,omitempty"`
	DecodeState   string          `json:"decodeState,omitempty"`
}

// hotfixRecordKey is the stable record identity used by page-identity cursors.
func hotfixRecordKey(r HotfixRecord) string {
	if r.ID != 0 {
		return "id:" + strconv.FormatUint(r.ID, 10)
	}
	table := uint64(0)
	if r.TableHash != nil {
		table = uint64(*r.TableHash)
	}
	return "push:" + strconv.FormatInt(int64(r.Push), 10) +
		":table:" + strconv.FormatUint(table, 10) +
		":record:" + strconv.FormatUint(uint64(r.RecordID), 10) +
		":status:" + strconv.FormatUint(uint64(r.Status), 10)
}

// HotfixFetchBudget is the visible network budget of one query run.
type HotfixFetchBudget struct {
	MaxRequests int   `json:"maxRequests"`
	MaxBytes    int64 `json:"maxBytes"`
	Requests    int   `json:"requests"`
	Bytes       int64 `json:"bytes"`
}

// HotfixCoverage describes what the query actually covered. Complete means the
// provider's own data for the query scope was fully covered; it never claims
// server-side coverage for a local snapshot.
type HotfixCoverage struct {
	Provider         string             `json:"provider"`
	Product          string             `json:"product"`
	FullBuild        string             `json:"fullBuild"`
	Region           string             `json:"region"`
	Locale           string             `json:"locale"`
	Complete         bool               `json:"complete"`
	Truncated        bool               `json:"truncated"`
	TruncationReason string             `json:"truncationReason,omitempty"`
	StartPage        int                `json:"startPage,omitempty"`
	EndPage          int                `json:"endPage,omitempty"`
	LastPage         int                `json:"lastPage,omitempty"`
	Total            int64              `json:"total,omitempty"`
	Scanned          int                `json:"scanned"`
	Matched          int                `json:"matched"`
	Returned         int                `json:"returned"`
	SelectedPush     *int32             `json:"selectedPush,omitempty"`
	CapturedAt       *time.Time         `json:"capturedAt,omitempty"`
	Ref              string             `json:"ref,omitempty"`
	Fetch            *HotfixFetchBudget `json:"fetch,omitempty"`
}

// RemoteHotfixResult is one provider run. Change is the ChangePin-compatible record
// (provider, product, build, region, query scope, fetch time, content digest).
type RemoteHotfixResult struct {
	Schema     string               `json:"schema"`
	Query      HotfixQuery          `json:"query"`
	Records    []HotfixRecord       `json:"records"`
	Coverage   HotfixCoverage       `json:"coverage"`
	Sources    []HotfixCoverage     `json:"sources,omitempty"`
	Warnings   []string             `json:"warnings,omitempty"`
	Change     selection.ChangePin  `json:"change"`
	Snapshot   string               `json:"snapshot,omitempty"`
	Capture    *evidence.CaptureRef `json:"capture,omitempty"`
	Complete   bool                 `json:"complete"`
	Truncated  bool                 `json:"truncated"`
	NextCursor string               `json:"nextCursor,omitempty"`
}

// HotfixProvider runs one shared query against one explicit source. Providers
// never fall back to another source by themselves.
type HotfixProvider interface {
	QueryHotfix(ctx context.Context, query HotfixQuery) (RemoteHotfixResult, error)
}

// HotfixCursorPage pins one remote page by its request identity and response
// bytes so a resumed window can refuse a shifted data set.
type HotfixCursorPage struct {
	Page        int    `json:"page"`
	Key         string `json:"key"`
	SHA256      string `json:"sha256"`
	Total       int64  `json:"total"`
	LastPage    int    `json:"lastPage"`
	NextPageURL string `json:"nextPageUrl,omitempty"`
}

// HotfixCursor is a page-identity cursor, never an offset into a changing data
// set: it names the continuation page, the last consumed record identity and
// the page identities it was cut from.
type HotfixCursor struct {
	Schema     string            `json:"schema"`
	Source     string            `json:"source"`
	Ref        string            `json:"ref"`
	Scope      string            `json:"scope"`
	ResumePage int               `json:"resumePage"`
	After      string            `json:"after,omitempty"`
	Previous   *HotfixCursorPage `json:"previous,omitempty"`
	Self       *HotfixCursorPage `json:"self,omitempty"`
	AfterIndex *uint32           `json:"afterIndex,omitempty"`
}

func EncodeHotfixCursor(cursor HotfixCursor) (string, error) {
	cursor.Schema = HotfixCursorSchema
	raw, err := json.Marshal(cursor)
	if err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(raw), nil
}

func DecodeHotfixCursor(encoded string) (HotfixCursor, error) {
	raw, err := base64.RawURLEncoding.DecodeString(encoded)
	if err != nil {
		return HotfixCursor{}, fmt.Errorf("%w: %v", ErrHotfixCursor, err)
	}
	var cursor HotfixCursor
	if err := json.Unmarshal(raw, &cursor); err != nil {
		return HotfixCursor{}, fmt.Errorf("%w: %v", ErrHotfixCursor, err)
	}
	if cursor.Schema != HotfixCursorSchema || cursor.Source == "" || len(cursor.Ref) != 64 {
		return HotfixCursor{}, fmt.Errorf("%w: malformed cursor", ErrHotfixCursor)
	}
	return cursor, nil
}

// hotfixScope is the canonical query scope recorded in cursors and ChangePin.
func (q HotfixQuery) hotfixScope() string {
	parts := []string{
		"product:" + q.Product,
		"build:" + q.FullBuild,
		"region:" + q.Region,
		"locale:" + q.Locale,
	}
	if q.Table != "" {
		parts = append(parts, "table:"+strings.ToLower(q.Table))
	}
	if q.TableHash != nil {
		parts = append(parts, "tableHash:"+strconv.FormatUint(uint64(*q.TableHash), 10))
	}
	if q.RecordID != nil {
		parts = append(parts, "record:"+strconv.FormatUint(uint64(*q.RecordID), 10))
	}
	if q.PushID != nil {
		parts = append(parts, "push:"+strconv.FormatInt(int64(*q.PushID), 10))
	}
	if q.RegionID != nil {
		parts = append(parts, "regionId:"+strconv.FormatUint(uint64(*q.RegionID), 10))
	}
	if q.Status != nil {
		parts = append(parts, "status:"+strconv.FormatUint(uint64(*q.Status), 10))
	}
	if q.From != nil {
		parts = append(parts, "from:"+q.From.UTC().Format(time.RFC3339Nano))
	}
	if q.To != nil {
		parts = append(parts, "to:"+q.To.UTC().Format(time.RFC3339Nano))
	}
	if q.Search != "" {
		parts = append(parts, "search:"+strings.ToLower(q.Search))
	}
	if q.Latest {
		parts = append(parts, "latest")
	}
	return strings.Join(parts, "|")
}

func hotfixScopeDigest(q HotfixQuery) string {
	sum := sha256.Sum256([]byte(q.hotfixScope()))
	return hex.EncodeToString(sum[:])
}

func hotfixDigest(parts ...string) string {
	sum := sha256.Sum256([]byte(strings.Join(parts, "\n")))
	return hex.EncodeToString(sum[:])
}

// hotfixMatchRecord applies the shared physical filters. Callers first enforce
// provider capabilities (ErrHotfixFilter) so an unevaluable filter is refused
// instead of silently matching nothing.
func (q HotfixQuery) hotfixMatchRecord(r HotfixRecord) bool {
	if q.RecordID != nil && r.RecordID != *q.RecordID {
		return false
	}
	if q.PushID != nil && r.Push != *q.PushID {
		return false
	}
	if q.Status != nil && r.Status != *q.Status {
		return false
	}
	if q.TableHash != nil {
		if r.TableHash == nil || *r.TableHash != *q.TableHash {
			return false
		}
	}
	if q.Table != "" {
		if r.TableName == "" || !strings.EqualFold(r.TableName, q.Table) {
			return false
		}
	}
	if q.RegionID != nil {
		if r.RegionID == nil || *r.RegionID != *q.RegionID {
			return false
		}
	}
	if r.Locale != "" && !strings.EqualFold(r.Locale, q.Locale) {
		return false
	}
	if !hotfixBuildMatches(q.FullBuild, r.Build) {
		return false
	}
	if q.From != nil || q.To != nil {
		created := wagoTimeValue(r.CreatedAt)
		if q.From != nil && created.Before(*q.From) {
			return false
		}
		if q.To != nil && created.After(*q.To) {
			return false
		}
	}
	return true
}

// wagoTimeValue parses the source-native timestamp; unparsable values stay zero
// and are then excluded by any explicit time range.
func wagoTimeValue(value string) time.Time {
	if value == "" {
		return time.Time{}
	}
	if parsed, err := ParseHotfixTime(value); err == nil {
		return parsed
	}
	return time.Time{}
}

// selectHotfixLatestBatch picks the largest signed push batch after filters and
// before pagination. Same-ID records are never folded together.
func selectHotfixLatestBatch(records []HotfixRecord) ([]HotfixRecord, *int32) {
	if len(records) == 0 {
		return records, nil
	}
	latest := records[0].Push
	for _, record := range records {
		if record.Push > latest {
			latest = record.Push
		}
	}
	batch := make([]HotfixRecord, 0, len(records))
	for _, record := range records {
		if record.Push == latest {
			batch = append(batch, record)
		}
	}
	return batch, &latest
}

// CacheFilterFromQuery maps the shared query onto the physical cache filter.
// Cache rows carry no names, timestamps or search text; those filters are
// refused instead of silently ignored. The caller must resolve table names to
// table hashes through pinned definitions first.
func CacheFilterFromQuery(q HotfixQuery) (CacheFilter, error) {
	build, err := hotfixBuildNumber(q.FullBuild)
	if err != nil {
		return CacheFilter{}, err
	}
	if q.Search != "" || q.From != nil || q.To != nil {
		return CacheFilter{}, fmt.Errorf("%w: cache rows carry no search text or timestamps", ErrHotfixFilter)
	}
	if q.Table != "" && q.TableHash == nil {
		return CacheFilter{}, fmt.Errorf("%w: cache rows carry table hashes; resolve the table name first", ErrHotfixFilter)
	}
	return CacheFilter{
		Build:     build,
		TableHash: q.TableHash,
		RecordID:  q.RecordID,
		Push:      q.PushID,
		Region:    q.RegionID,
		Status:    q.Status,
		Latest:    q.Latest,
		Limit:     q.Limit,
	}, nil
}

// HotfixChange builds the ChangePin-compatible identity record of one run.
func HotfixChange(provider string, q HotfixQuery, fetchedAt time.Time, digest string) selection.ChangePin {
	return selection.ChangePin{
		Provider:  provider,
		Product:   q.Product,
		FullBuild: q.FullBuild,
		Region:    q.Region,
		Scope:     provider + ":" + q.hotfixScope(),
		FetchedAt: fetchedAt,
		SHA256:    digest,
	}
}

// PinHotfixChange publishes a ChangePin-compatible record as a derived
// selection set. Existing pinned changes are preserved: an identical change
// reuses the parent set and a different one is refused.
func PinHotfixChange(ctx context.Context, root, parent string, change selection.ChangePin) (selection.PinnedSet, error) {
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (selection.PinnedSet, error) {
		return pinHotfixChange(ctx, selection.OpenPinner(metadata), parent, change)
	})
}

func pinHotfixChange(ctx context.Context, pinner *selection.Pinner, parent string, change selection.ChangePin) (selection.PinnedSet, error) {
	if parent == "" {
		return pinner.PinSelection(ctx, selection.SelectionSpec{Changes: &change})
	}
	existing, err := pinner.ReadPinnedSet(ctx, parent)
	if err != nil {
		return selection.PinnedSet{}, err
	}
	if existing.Changes != nil {
		if *existing.Changes != change {
			return selection.PinnedSet{}, fmt.Errorf("%w: parent already pins a different change", ErrCacheIdentity)
		}
		return existing, nil
	}
	return pinner.PinSelection(ctx, selection.SelectionSpec{Parent: parent, Changes: &change})
}
