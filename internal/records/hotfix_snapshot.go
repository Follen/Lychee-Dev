// SPDX-License-Identifier: MIT
package records

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

const (
	dbcacheProviderName  = "dbcache"
	raidbotsProviderName = "raidbots"
	// RaidbotsSnapshotMaxAge refuses snapshots older than 30 days.
	RaidbotsSnapshotMaxAge = 30 * 24 * time.Hour
)

// SnapshotHotfix reads one explicitly supplied DBCache snapshot. The file
// attests only its build number and capture time (file mtime); product, region
// and locale stay the caller's declared context and must match the query.
type SnapshotHotfix struct {
	File      string
	Provider  string // "dbcache" or "raidbots"
	Product   string
	FullBuild string
	Region    string
	Locale    string
	MaxAge    time.Duration
	MaxBytes  int64
}

// OpenHotfixSnapshot declares an explicit snapshot source with a staleness
// policy. A zero MaxAge means no staleness refusal.
func OpenHotfixSnapshot(file, provider string, q HotfixQuery, maxAge time.Duration) (SnapshotHotfix, error) {
	if err := q.Validate(); err != nil {
		return SnapshotHotfix{}, err
	}
	if provider != dbcacheProviderName && provider != raidbotsProviderName {
		return SnapshotHotfix{}, fmt.Errorf("%w: snapshot provider must be dbcache or raidbots", ErrHotfixQuery)
	}
	if file == "" {
		return SnapshotHotfix{}, fmt.Errorf("%w: snapshot file is required", ErrHotfixQuery)
	}
	return SnapshotHotfix{
		File:      file,
		Provider:  provider,
		Product:   q.Product,
		FullBuild: q.FullBuild,
		Region:    q.Region,
		Locale:    q.Locale,
		MaxAge:    maxAge,
	}, nil
}

// RaidbotsSnapshot declares a captured DBCache snapshot (for example a Raidbots
// download) refused when older than 30 days. Its coverage never claims
// server-side completeness.
func RaidbotsSnapshot(file string, q HotfixQuery) (SnapshotHotfix, error) {
	return OpenHotfixSnapshot(file, raidbotsProviderName, q, RaidbotsSnapshotMaxAge)
}

// DBCacheSnapshot declares the explicitly supplied client cache file. There is
// no staleness policy because the live client file may be queried at any time;
// the capture time is still reported.
func DBCacheSnapshot(file string, q HotfixQuery) (SnapshotHotfix, error) {
	return OpenHotfixSnapshot(file, dbcacheProviderName, q, 0)
}

func (s SnapshotHotfix) QueryHotfix(ctx context.Context, q HotfixQuery) (RemoteHotfixResult, error) {
	if err := q.Validate(); err != nil {
		return RemoteHotfixResult{}, err
	}
	if s.Product != q.Product || s.FullBuild != q.FullBuild || s.Region != q.Region || s.Locale != q.Locale {
		return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot declares %s %s %s %s, query asks %s %s %s %s", ErrCacheIdentity,
			s.Product, s.FullBuild, s.Region, s.Locale, q.Product, q.FullBuild, q.Region, q.Locale)
	}
	if q.Search != "" || q.From != nil || q.To != nil {
		return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot rows carry no search text or timestamps", ErrHotfixFilter)
	}
	if q.Page != 0 {
		return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot pagination uses the immutable after-index cursor", ErrHotfixFilter)
	}
	filter, err := CacheFilterFromQuery(q)
	if err != nil {
		return RemoteHotfixResult{}, err
	}
	var cursor *HotfixCursor
	if q.Cursor != "" {
		decoded, err := DecodeHotfixCursor(q.Cursor)
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		if decoded.Source != s.Provider {
			return RemoteHotfixResult{}, fmt.Errorf("%w: cursor was cut from %q, not %q", ErrHotfixCursor, decoded.Source, s.Provider)
		}
		if decoded.Scope != hotfixScopeDigest(q) {
			return RemoteHotfixResult{}, fmt.Errorf("%w: query scope differs from the cursor scope", ErrHotfixCursor)
		}
		cursor = &decoded
	}
	info, err := os.Stat(s.File)
	if err != nil {
		return RemoteHotfixResult{}, err
	}
	if !info.Mode().IsRegular() {
		return RemoteHotfixResult{}, ErrCacheFormat
	}
	capturedAt := info.ModTime().UTC()
	if s.MaxAge > 0 && time.Since(capturedAt) > s.MaxAge {
		return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot captured at %s is older than %s", ErrHotfixSnapshotStale, capturedAt.Format(time.RFC3339), s.MaxAge)
	}
	maxBytes := s.MaxBytes
	if maxBytes <= 0 {
		maxBytes = 512 << 20
	}
	raw, err := readCacheFile(ctx, s.File, maxBytes)
	if err != nil {
		return RemoteHotfixResult{}, err
	}
	sum := sha256.Sum256(raw)
	digest := hex.EncodeToString(sum[:])
	if cursor != nil {
		if cursor.Ref != digest {
			return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot bytes changed since the cursor was cut", ErrHotfixCursor)
		}
		if cursor.AfterIndex == nil {
			return RemoteHotfixResult{}, fmt.Errorf("%w: snapshot cursor has no after-index", ErrHotfixCursor)
		}
		filter.AfterIndex = cursor.AfterIndex
	}
	page, err := InspectCache(ctx, raw, filter)
	if err != nil {
		return RemoteHotfixResult{}, err
	}
	if q.Definition != nil {
		if err := applyCacheDecode(ctx, raw, &page, *q.Definition); err != nil {
			return RemoteHotfixResult{}, err
		}
	}
	records := hotfixRecordsFromCache(s.Provider, page)
	nextCursor := ""
	if page.Truncated && page.NextIndex != nil {
		encoded, err := EncodeHotfixCursor(HotfixCursor{
			Source:     s.Provider,
			Ref:        digest,
			Scope:      hotfixScopeDigest(q),
			AfterIndex: page.NextIndex,
		})
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		nextCursor = encoded
	}
	complete := page.Complete
	coverage := HotfixCoverage{
		Provider:  s.Provider,
		Product:   q.Product,
		FullBuild: q.FullBuild,
		Region:    q.Region,
		Locale:    q.Locale,
		// A snapshot may lag the server; only a live cache file attests that its
		// own full data set was covered, never server-side coverage.
		Complete:          s.Provider == dbcacheProviderName && page.Complete,
		FullQueryComplete: s.Provider == dbcacheProviderName && page.Complete && cursor == nil,
		Truncated:         page.Truncated,
		Scanned:           int(page.Scanned),
		Matched:           int(page.Matched),
		Returned:          len(records),
		SelectedPush:      page.SelectedPush,
		CapturedAt:        &capturedAt,
		Ref:               digest,
	}
	if page.Truncated {
		coverage.TruncationReason = "limit"
	}
	return RemoteHotfixResult{
		Schema:     RemoteHotfixResultSchema,
		Query:      q,
		Records:    records,
		Coverage:   coverage,
		Change:     HotfixChange(s.Provider, q, capturedAt, digest),
		Complete:   complete,
		Truncated:  page.Truncated,
		NextCursor: nextCursor,
	}, nil
}

func hotfixRecordsFromCache(provider string, page CachePage) []HotfixRecord {
	records := make([]HotfixRecord, 0, len(page.Entries))
	build := fmt.Sprint(page.Filter.Build)
	for _, entry := range page.Entries {
		index := entry.Index
		table := entry.TableHash
		records = append(records, HotfixRecord{
			Provider:      provider,
			Push:          entry.Push,
			UniqueID:      entry.UniqueID,
			RegionID:      entry.Region,
			TableHash:     &table,
			RecordID:      entry.RecordID,
			Status:        entry.Status,
			Build:         build,
			Index:         &index,
			PayloadLength: entry.PayloadBytes,
			PayloadHex:    entry.PayloadHex,
			Fields:        entry.Fields,
			DecodeState:   entry.DecodeState,
		})
	}
	return records
}

// applyCacheDecode mirrors the local use case's decode rules exactly: no_payload
// without bytes, not_valid when the raw status is not a valid row for the
// record layout, decoded otherwise. Cache payloads are sequential XFTH bytes.
func applyCacheDecode(ctx context.Context, raw []byte, page *CachePage, definition schema.Definition) error {
	fieldsBytes := 0
	for i := range page.Entries {
		entry := &page.Entries[i]
		entry.DecodeState = "no_payload"
		if entry.PayloadBytes == 0 {
			continue
		}
		if page.RecordLayout >= 7 && entry.Status != 1 || page.RecordLayout < 7 && entry.Status == 0 {
			entry.DecodeState = "not_valid"
			continue
		}
		fields, err := DecodeHotfixFields(ctx, raw[int(entry.PayloadOffset):int(entry.PayloadOffset)+entry.PayloadBytes], entry.RecordID, definition)
		if err != nil {
			return fmt.Errorf("record index %d: %w", entry.Index, err)
		}
		serialized, err := json.Marshal(fields)
		if err != nil {
			return err
		}
		if len(serialized) > (8<<20)-fieldsBytes {
			return ErrCacheLimit
		}
		fieldsBytes += len(serialized)
		entry.Fields = fields
		entry.DecodeState = "decoded"
	}
	return nil
}
