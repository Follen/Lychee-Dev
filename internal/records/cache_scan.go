// SPDX-License-Identifier: AGPL-3.0-or-later
// XFTH layouts adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
)

var ErrCacheFormat = errors.New("records.hotfix_format")
var ErrCacheVersion = errors.New("records.hotfix_version")
var ErrCacheBuild = errors.New("records.hotfix_build_mismatch")
var ErrCacheLimit = errors.New("records.hotfix_limit")
var ErrCacheIdentity = errors.New("records.hotfix_identity")

// CacheFilter selects physical records, not an effective DB2 overlay. Repeated
// record IDs, invalidations, unknown status values and negative pushes survive.
type CacheFilter struct {
	Build      uint32  `json:"build"`
	TableHash  *uint32 `json:"tableHash,omitempty"`
	RecordID   *uint32 `json:"recordId,omitempty"`
	Push       *int32  `json:"push,omitempty"`
	Region     *uint32 `json:"region,omitempty"`
	Status     *uint8  `json:"status,omitempty"`
	Latest     bool    `json:"latest,omitempty"`
	AfterIndex *uint32 `json:"afterIndex,omitempty"`
	Limit      int     `json:"limit"`
}

type CacheEntry struct {
	Index         uint32         `json:"index"`
	Region        *uint32        `json:"region,omitempty"`
	Push          int32          `json:"push"`
	UniqueID      *uint32        `json:"uniqueId,omitempty"`
	TableHash     uint32         `json:"tableHash"`
	RecordID      uint32         `json:"recordId"`
	Status        uint8          `json:"status"`
	PayloadOffset int64          `json:"payloadOffset"`
	PayloadBytes  int            `json:"payloadBytes"`
	PayloadHex    string         `json:"payloadHex"`
	Fields        map[string]any `json:"fields,omitempty"`
	DecodeState   string         `json:"decodeState,omitempty"`
}

type CachePage struct {
	Version      uint32       `json:"version"`
	RecordLayout uint32       `json:"recordLayout"`
	Filter       CacheFilter  `json:"filter"`
	Entries      []CacheEntry `json:"entries"`
	Scanned      uint32       `json:"scanned"`
	Matched      uint32       `json:"matched"`
	SelectedPush *int32       `json:"selectedPush,omitempty"`
	NextIndex    *uint32      `json:"nextIndex,omitempty"`
	Complete     bool         `json:"complete"`
	Truncated    bool         `json:"truncated"`
}

// InspectCache validates the entire byte snapshot before returning a page. A
// corrupt tail never becomes a successful limited query. At most 8 MiB of raw
// payload is materialized per page (16 MiB after hex encoding).
func InspectCache(ctx context.Context, raw []byte, filter CacheFilter) (CachePage, error) {
	if err := ctx.Err(); err != nil {
		return CachePage{}, err
	}
	if filter.Limit < 1 || filter.Limit > 200 || len(raw) > 512<<20 {
		return CachePage{}, ErrCacheLimit
	}
	if len(raw) < 12 || string(raw[:4]) != "XFTH" {
		return CachePage{}, ErrCacheFormat
	}
	u32 := binary.LittleEndian.Uint32
	version, build := u32(raw[4:8]), u32(raw[8:12])
	if version < 1 || version > 9 {
		return CachePage{}, ErrCacheVersion
	}
	if build == 0 || build > 0x7fffffff || filter.Build != build {
		return CachePage{}, ErrCacheBuild
	}
	start := 12
	if version >= 5 {
		start = 44
	}
	if len(raw) < start {
		return CachePage{}, ErrCacheFormat
	}
	layout := version
	if version == 8 && len(raw) > start {
		// Both layouts were shipped under version 8. Check complete framing, not
		// just the next magic (which can also occur inside a payload).
		a := walkCache(ctx, raw, start, 8, nil)
		b := walkCache(ctx, raw, start, 7, nil)
		if err := ctx.Err(); err != nil {
			return CachePage{}, err
		}
		if errors.Is(a, ErrCacheLimit) || errors.Is(b, ErrCacheLimit) {
			return CachePage{}, ErrCacheLimit
		}
		if a == nil && b == nil {
			return CachePage{}, fmt.Errorf("%w: ambiguous version 8 layout", ErrCacheFormat)
		}
		if a != nil && b != nil {
			return CachePage{}, ErrCacheFormat
		}
		if a != nil {
			layout = 7
		}
	}
	if filter.Region != nil && layout < 9 {
		return CachePage{}, fmt.Errorf("%w: record layout %d carries no region", ErrHotfixFilter, layout)
	}
	page := CachePage{Version: version, RecordLayout: layout, Filter: filter, Entries: []CacheEntry{}}
	var latestPush int32
	haveLatest := false
	if filter.Latest {
		if err := walkCache(ctx, raw, start, layout, func(entry CacheEntry) error {
			page.Scanned++
			if !cacheEntryMatches(filter, entry) {
				return nil
			}
			if !haveLatest || entry.Push > latestPush {
				latestPush = entry.Push
				haveLatest = true
			}
			return nil
		}); err != nil {
			return CachePage{}, err
		}
		if !haveLatest {
			page.Complete = filter.AfterIndex == nil
			return page, nil
		}
		page.SelectedPush = &latestPush
	}
	payloadBytes := 0
	err := walkCache(ctx, raw, start, layout, func(entry CacheEntry) error {
		if !filter.Latest {
			page.Scanned++
		}
		if !cacheEntryMatches(filter, entry) {
			return nil
		}
		if filter.Latest {
			if entry.Push != latestPush {
				return nil
			}
		}
		page.Matched++
		if filter.AfterIndex != nil && entry.Index <= *filter.AfterIndex {
			return nil
		}
		if len(page.Entries) == filter.Limit {
			page.Truncated = true
			return nil
		}
		if entry.PayloadBytes > (8<<20)-payloadBytes {
			return ErrCacheLimit
		}
		payloadBytes += entry.PayloadBytes
		entry.PayloadHex = hex.EncodeToString(raw[int(entry.PayloadOffset) : int(entry.PayloadOffset)+entry.PayloadBytes])
		page.Entries = append(page.Entries, entry)
		return nil
	})
	if err != nil {
		return CachePage{}, err
	}
	if page.Truncated {
		last := page.Entries[len(page.Entries)-1].Index
		page.NextIndex = &last
	}
	page.Complete = filter.AfterIndex == nil && !page.Truncated
	return page, nil
}

// cacheEntryMatches applies the shared physical filters. A nil filter field
// means no constraint; repeated IDs, invalidations and negative pushes survive.
func cacheEntryMatches(filter CacheFilter, entry CacheEntry) bool {
	if filter.TableHash != nil && entry.TableHash != *filter.TableHash {
		return false
	}
	if filter.RecordID != nil && entry.RecordID != *filter.RecordID {
		return false
	}
	if filter.Push != nil && entry.Push != *filter.Push {
		return false
	}
	if filter.Region != nil && (entry.Region == nil || *entry.Region != *filter.Region) {
		return false
	}
	if filter.Status != nil && entry.Status != *filter.Status {
		return false
	}
	return true
}

// Layout offsets follow XFTH versions 1-9 (wowdata / DBCD); no old runtime,
// index, effective-row policy or implicit latest-record selection is reused.
func walkCache(ctx context.Context, raw []byte, start int, layout uint32, yield func(CacheEntry) error) error {
	width := 28
	if layout == 1 || layout == 7 {
		width = 24
	}
	if layout == 9 {
		width = 32
	}
	u32 := binary.LittleEndian.Uint32
	var index uint32
	for offset := start; offset < len(raw); index++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		if index >= 8_000_000 {
			return ErrCacheLimit
		}
		if len(raw)-offset < width || string(raw[offset:offset+4]) != "XFTH" {
			return ErrCacheFormat
		}
		h := raw[offset : offset+width]
		e := CacheEntry{Index: index, PayloadOffset: int64(offset + width)}
		var size uint32
		switch layout {
		case 1:
			e.Push, size, e.TableHash, e.RecordID, e.Status = int32(u32(h[4:8])), u32(h[8:12]), u32(h[12:16]), u32(h[16:20]), h[20]
		case 2, 3, 4, 5, 6:
			e.Push, size, e.TableHash, e.RecordID, e.Status = int32(u32(h[8:12])), u32(h[12:16]), u32(h[16:20]), u32(h[20:24]), h[24]
		case 7:
			e.Push, e.TableHash, e.RecordID, size, e.Status = int32(u32(h[4:8])), u32(h[8:12]), u32(h[12:16]), u32(h[16:20]), h[20]
		case 8:
			unique := u32(h[8:12])
			e.UniqueID = &unique
			e.Push, e.TableHash, e.RecordID, size, e.Status = int32(u32(h[4:8])), u32(h[12:16]), u32(h[16:20]), u32(h[20:24]), h[24]
		case 9:
			region, unique := u32(h[4:8]), u32(h[12:16])
			e.Region, e.UniqueID = &region, &unique
			e.Push, e.TableHash, e.RecordID, size, e.Status = int32(u32(h[8:12])), u32(h[16:20]), u32(h[20:24]), u32(h[24:28]), h[28]
		}
		if size > 0x7fffffff || uint64(size) > uint64(len(raw)-offset-width) {
			return ErrCacheFormat
		}
		e.PayloadBytes = int(size)
		if yield != nil {
			if err := yield(e); err != nil {
				return err
			}
		}
		offset += width + int(size)
	}
	return ctx.Err()
}
