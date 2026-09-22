// SPDX-License-Identifier: AGPL-3.0-or-later
// Wago hotfix provider adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const (
	wagoProviderName    = "wago"
	wagoDefaultBaseURL  = "https://wago.tools"
	wagoMaxPages        = 2048
	wagoMaxPageBytes    = 16 << 20
	wagoDefaultMaxBytes = 256 << 20
)

// errWagoFetchBudget is an internal walk sentinel: the visible limit stopped
// further fetching without corrupting the already collected window.
var errWagoFetchBudget = errors.New("records: wago fetch budget reached")

// WagoHotfix queries https://wago.tools/hotfixes pages with strict parsing,
// per-page workspace receipts and explicit budgets. It performs no silent
// source fallback and never reads the network in offline mode.
type WagoHotfix struct {
	Root     string
	BaseURL  string
	Client   *http.Client
	Offline  bool
	MaxPages int
	Budget   HotfixFetchBudget
}

// WagoHotfixRequest is the use-case request for QueryWagoHotfix. Fallback is
// consulted only per the documented composition rules and is always reported.
type WagoHotfixRequest struct {
	Query    HotfixQuery
	BaseURL  string
	Client   *http.Client
	Offline  bool
	MaxPages int
	Budget   HotfixFetchBudget
	Parent   string
	Fallback *SnapshotHotfix
}

type wagoWalkPage struct {
	identity   wagoPageIdentity
	page       WagoPage
	sha        string
	capturedAt time.Time
	fromCache  bool
}

type wagoFetchState struct {
	budget HotfixFetchBudget
}

type wagoMatch struct {
	record HotfixRecord
	page   int
}

func (w *WagoHotfix) QueryHotfix(ctx context.Context, q HotfixQuery) (RemoteHotfixResult, error) {
	if err := q.Validate(); err != nil {
		return RemoteHotfixResult{}, err
	}
	if q.TableHash != nil {
		return RemoteHotfixResult{}, fmt.Errorf("%w: wago rows carry table names; select the table name instead", ErrHotfixFilter)
	}
	maxPages := w.MaxPages
	if maxPages == 0 {
		maxPages = wagoMaxPages
	}
	if maxPages < 1 || maxPages > wagoMaxPages {
		return RemoteHotfixResult{}, fmt.Errorf("%w: max pages must be 1..%d", ErrHotfixBudget, wagoMaxPages)
	}
	budget := w.Budget
	if budget.MaxRequests == 0 {
		budget.MaxRequests = maxPages
	}
	if budget.MaxBytes == 0 {
		budget.MaxBytes = wagoDefaultMaxBytes
	}
	if budget.MaxRequests < 1 || budget.MaxBytes < 1 {
		return RemoteHotfixResult{}, fmt.Errorf("%w: budgets must be positive", ErrHotfixBudget)
	}
	var cursor *HotfixCursor
	if q.Cursor != "" {
		decoded, err := DecodeHotfixCursor(q.Cursor)
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		if decoded.Source != wagoProviderName {
			return RemoteHotfixResult{}, fmt.Errorf("%w: cursor was cut from %q, not wago", ErrHotfixCursor, decoded.Source)
		}
		if decoded.Scope != hotfixScopeDigest(q) {
			return RemoteHotfixResult{}, fmt.Errorf("%w: query scope differs from the cursor scope", ErrHotfixCursor)
		}
		cursor = &decoded
	}
	startPage := q.Page
	if startPage == 0 {
		startPage = 1
	}
	resumeAfter := ""
	var self *HotfixCursorPage
	metaByPage := map[int]HotfixCursorPage{}
	if cursor != nil {
		if cursor.Self != nil {
			metaByPage[cursor.Self.Page] = *cursor.Self
		}
		if cursor.Previous != nil {
			metaByPage[cursor.Previous.Page] = *cursor.Previous
		}
		resumeAfter = cursor.After
		if !q.Latest {
			if q.Page != 0 {
				return RemoteHotfixResult{}, fmt.Errorf("%w: a resume cursor already fixes the page position", ErrHotfixQuery)
			}
			startPage = cursor.ResumePage
			self = cursor.Self
		}
	}
	if startPage < 1 {
		return RemoteHotfixResult{}, fmt.Errorf("%w: cursor has no resume page", ErrHotfixCursor)
	}

	search := wagoEffectiveSearch(q)
	state := &wagoFetchState{budget: HotfixFetchBudget{MaxRequests: budget.MaxRequests, MaxBytes: budget.MaxBytes}}
	chain := wagoChainStart
	if cursor != nil {
		chain = cursor.Ref
	}

	var (
		windowed     []wagoMatch
		matchedCount int
		scanned      int
		walked       []wagoWalkPage
		warnings     []string
		unknownState = map[uint8]bool{}
		seenIDs      = map[uint64]bool{}
		consumed     = resumeAfter == ""
		stopReason   string
	)
	pageNumber := startPage
	for {
		if err := ctx.Err(); err != nil {
			return RemoteHotfixResult{}, err
		}
		if len(walked) >= maxPages {
			stopReason = "page_budget"
			break
		}
		identity := wagoPageIdentity{
			Provider:  wagoProviderName,
			Parser:    WagoParserVersion,
			Product:   q.Product,
			FullBuild: q.FullBuild,
			Region:    q.Region,
			Locale:    q.Locale,
			Search:    search,
			Page:      pageNumber,
		}
		fetched, err := w.readPage(ctx, identity, state)
		if errors.Is(err, errWagoFetchBudget) {
			stopReason = "page_budget"
			break
		}
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		if len(walked) == 0 {
			if self != nil {
				if fetched.sha != self.SHA256 || fetched.identity.key() != self.Key {
					return RemoteHotfixResult{}, fmt.Errorf("%w: resumed page %d bytes changed", ErrHotfixPageDrift, pageNumber)
				}
			} else if previous, ok := metaByPage[fetched.page.CurrentPage-1]; ok {
				if err := ValidateWagoPageSequence(wagoPageFromMeta(previous), fetched.page); err != nil {
					return RemoteHotfixResult{}, err
				}
			}
			if q.Latest {
				remaining := fetched.page.LastPage - pageNumber + 1
				if remaining > maxPages {
					return RemoteHotfixResult{}, fmt.Errorf("%w: candidate set requires %d pages from page %d, limit is %d", ErrHotfixBudget, remaining, pageNumber, maxPages)
				}
			}
		} else if err := ValidateWagoPageSequence(walked[len(walked)-1].page, fetched.page); err != nil {
			return RemoteHotfixResult{}, err
		}
		for _, record := range fetched.page.Records {
			if record.ID == 0 {
				continue
			}
			if seenIDs[record.ID] {
				return RemoteHotfixResult{}, fmt.Errorf("%w: record id %d repeats across pages", ErrHotfixPageDrift, record.ID)
			}
			seenIDs[record.ID] = true
		}
		scanned += len(fetched.page.Records)
		windowFull := false
		for _, record := range fetched.page.Records {
			if record.Status < 1 || record.Status > 4 {
				unknownState[record.Status] = true
			}
			converted := hotfixRecordFromWago(record)
			if !q.hotfixMatchRecord(converted) {
				continue
			}
			matchedCount++
			key := hotfixRecordKey(converted)
			if !consumed {
				if key == resumeAfter {
					consumed = true
				}
				continue
			}
			windowed = append(windowed, wagoMatch{record: converted, page: pageNumber})
			if !q.Latest && len(windowed) > q.Limit {
				windowFull = true
				break
			}
		}
		walked = append(walked, fetched)
		metaByPage[fetched.page.CurrentPage] = wagoMetaFor(fetched)
		chain = hotfixDigest(chain, fetched.identity.key(), fetched.sha)
		if windowFull {
			stopReason = "limit"
			break
		}
		if pageNumber == fetched.page.LastPage {
			break
		}
		pageNumber++
	}
	if len(walked) == 0 {
		return RemoteHotfixResult{}, fmt.Errorf("%w: no page could be read", ErrHotfixBudget)
	}
	if !consumed {
		return RemoteHotfixResult{}, fmt.Errorf("%w: cursor record %q is not in the resumed source", ErrHotfixCursor, resumeAfter)
	}

	records := make([]HotfixRecord, 0, len(windowed))
	var selectedPush *int32
	if q.Latest {
		if stopReason == "page_budget" {
			return RemoteHotfixResult{}, fmt.Errorf("%w: the latest batch needs the full candidate range", ErrHotfixBudget)
		}
		batch := make([]HotfixRecord, 0, len(windowed))
		for _, match := range windowed {
			batch = append(batch, match.record)
		}
		batch, selectedPush = selectHotfixLatestBatch(batch)
		if len(batch) > q.Limit {
			stopReason = "limit"
			batch = batch[:q.Limit]
		} else {
			stopReason = ""
		}
		records = batch
	} else if stopReason == "limit" {
		records = make([]HotfixRecord, 0, q.Limit)
		for _, match := range windowed[:q.Limit] {
			records = append(records, match.record)
		}
	} else {
		for _, match := range windowed {
			records = append(records, match.record)
		}
	}

	if q.Definition != nil {
		if err := applyWagoDecode(records, *q.Definition); err != nil {
			return RemoteHotfixResult{}, err
		}
	}

	last := walked[len(walked)-1]
	for status := range unknownState {
		warnings = append(warnings, fmt.Sprintf("records.hotfix_unknown_status:%d", status))
	}
	sort.Strings(warnings)
	cacheHits := 0
	for _, page := range walked {
		if page.fromCache {
			cacheHits++
		}
	}
	if cacheHits == len(walked) {
		warnings = append(warnings, "records.wago_cache_hit")
	}
	if matchedCount == 0 && scanned > 0 {
		warnings = append(warnings, "records.wago_search_miss")
	}

	truncated := stopReason != ""
	nextCursor := ""
	if truncated {
		cursorPage, previousPage := wagoCursorPositions(windowed, records, stopReason, last, metaByPage)
		encoded, err := EncodeHotfixCursor(HotfixCursor{
			Source:     wagoProviderName,
			Ref:        chain,
			Scope:      hotfixScopeDigest(q),
			ResumePage: cursorPage.page,
			After:      cursorPage.after,
			Previous:   previousPage,
			Self:       cursorPage.self,
		})
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		nextCursor = encoded
	}

	capturedAt := walked[0].capturedAt
	for _, page := range walked {
		if page.capturedAt.After(capturedAt) {
			capturedAt = page.capturedAt
		}
	}
	complete := !truncated && walked[len(walked)-1].page.CurrentPage == walked[len(walked)-1].page.LastPage
	coverage := HotfixCoverage{
		Provider:         wagoProviderName,
		Product:          q.Product,
		FullBuild:        q.FullBuild,
		Region:           q.Region,
		Locale:           q.Locale,
		Complete:         complete,
		Truncated:        truncated,
		TruncationReason: stopReason,
		StartPage:        startPage,
		EndPage:          last.page.CurrentPage,
		LastPage:         last.page.LastPage,
		Total:            last.page.Total,
		Scanned:          scanned,
		Matched:          matchedCount,
		Returned:         len(records),
		SelectedPush:     selectedPush,
		CapturedAt:       &capturedAt,
		Ref:              chain,
		Fetch:            &state.budget,
	}
	return RemoteHotfixResult{
		Schema:     RemoteHotfixResultSchema,
		Query:      q,
		Records:    records,
		Coverage:   coverage,
		Warnings:   warnings,
		Change:     HotfixChange(wagoProviderName, q, capturedAt, chain),
		Complete:   complete,
		Truncated:  truncated,
		NextCursor: nextCursor,
	}, nil
}

type wagoCursorPosition struct {
	page  int
	after string
	self  *HotfixCursorPage
}

// wagoCursorPositions names the continuation by page identity and record
// identity, never by an offset into the changing remote set.
func wagoCursorPositions(windowed []wagoMatch, records []HotfixRecord, stopReason string, last wagoWalkPage, metaByPage map[int]HotfixCursorPage) (wagoCursorPosition, *HotfixCursorPage) {
	if stopReason == "limit" && len(records) > 0 {
		lastKey := hotfixRecordKey(records[len(records)-1])
		for _, match := range windowed {
			if hotfixRecordKey(match.record) != lastKey {
				continue
			}
			cursorPage := wagoCursorPosition{page: match.page, after: lastKey}
			if selfValue, ok := metaByPage[match.page]; ok {
				selfCopy := selfValue
				cursorPage.self = &selfCopy
			}
			var previous *HotfixCursorPage
			if prior, ok := metaByPage[match.page-1]; ok {
				priorCopy := prior
				previous = &priorCopy
			}
			return cursorPage, previous
		}
	}
	// The walk budget stopped the scan at a page boundary: resume at the next
	// page and pin the last consumed page's identity keys for drift checks.
	lastMeta := wagoMetaFor(last)
	return wagoCursorPosition{page: last.page.CurrentPage + 1}, &lastMeta
}

func wagoMetaFor(page wagoWalkPage) HotfixCursorPage {
	return HotfixCursorPage{
		Page:        page.page.CurrentPage,
		Key:         page.identity.key(),
		SHA256:      page.sha,
		Total:       page.page.Total,
		LastPage:    page.page.LastPage,
		NextPageURL: page.page.NextPageURL,
	}
}

func wagoPageFromMeta(meta HotfixCursorPage) WagoPage {
	return WagoPage{
		CurrentPage: meta.Page,
		LastPage:    meta.LastPage,
		Total:       meta.Total,
		NextPageURL: meta.NextPageURL,
	}
}

const wagoChainStart = "lycheedev.hotfix-cursor.v1"

// readPage returns one verified page from the workspace receipt cache or the
// network. Offline mode only reads verified receipts and refuses the network.
func (w *WagoHotfix) readPage(ctx context.Context, identity wagoPageIdentity, state *wagoFetchState) (wagoWalkPage, error) {
	receipt, raw, err := loadWagoPage(ctx, w.Root, identity)
	switch {
	case err == nil:
		page, parseErr := ParseWagoPage(raw)
		if parseErr != nil {
			return wagoWalkPage{}, fmt.Errorf("%w: %v", ErrHotfixCacheCorrupt, parseErr)
		}
		if page.CurrentPage != identity.Page {
			return wagoWalkPage{}, fmt.Errorf("%w: cached page %d stored for page %d", ErrHotfixCacheCorrupt, page.CurrentPage, identity.Page)
		}
		sum := receipt.Response.SHA256
		return wagoWalkPage{identity: identity, page: page, sha: sum, capturedAt: receipt.CapturedAt, fromCache: true}, nil
	case errors.Is(err, ErrHotfixCacheMiss):
	default:
		return wagoWalkPage{}, err
	}
	if w.Offline {
		return wagoWalkPage{}, fmt.Errorf("%w: page %d is not in the verified cache", ErrHotfixOffline, identity.Page)
	}
	if state.budget.Requests >= state.budget.MaxRequests || state.budget.Bytes >= state.budget.MaxBytes {
		return wagoWalkPage{}, errWagoFetchBudget
	}
	requestURL, err := wagoPageURL(w.baseURL(), identity.Search, identity.Page)
	if err != nil {
		return wagoWalkPage{}, err
	}
	raw, capturedAt, err := w.fetchPage(ctx, requestURL, state)
	if err != nil {
		return wagoWalkPage{}, err
	}
	page, err := ParseWagoPage(raw)
	if err != nil {
		return wagoWalkPage{}, err
	}
	if page.CurrentPage != identity.Page {
		return wagoWalkPage{}, fmt.Errorf("%w: server returned page %d for page %d", ErrHotfixPageDrift, page.CurrentPage, identity.Page)
	}
	receipt, err = storeWagoPage(ctx, w.Root, identity, requestURL, raw, page, capturedAt)
	if err != nil {
		return wagoWalkPage{}, err
	}
	return wagoWalkPage{identity: identity, page: page, sha: receipt.Response.SHA256, capturedAt: capturedAt}, nil
}

func (w *WagoHotfix) baseURL() string {
	if strings.TrimSpace(w.BaseURL) == "" {
		return wagoDefaultBaseURL
	}
	return strings.TrimRight(w.BaseURL, "/")
}

func (w *WagoHotfix) fetchPage(ctx context.Context, requestURL string, state *wagoFetchState) ([]byte, time.Time, error) {
	if err := ctx.Err(); err != nil {
		return nil, time.Time{}, err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, requestURL, nil)
	if err != nil {
		return nil, time.Time{}, err
	}
	request.Header.Set("Accept", "text/html,application/xhtml+xml")
	client := w.Client
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Second}
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, time.Time{}, fmt.Errorf("%w: %v", ErrHotfixWagoHTTP, err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, time.Time{}, fmt.Errorf("%w: status %d", ErrHotfixWagoHTTP, response.StatusCode)
	}
	raw, err := io.ReadAll(io.LimitReader(&metadataReader{ctx: ctx, source: response.Body}, wagoMaxPageBytes+1))
	if err != nil {
		return nil, time.Time{}, fmt.Errorf("%w: %v", ErrHotfixWagoHTTP, err)
	}
	if int64(len(raw)) > wagoMaxPageBytes {
		return nil, time.Time{}, fmt.Errorf("%w: page exceeds %d bytes", ErrHotfixBudget, wagoMaxPageBytes)
	}
	state.budget.Requests++
	state.budget.Bytes += int64(len(raw))
	if state.budget.Bytes > state.budget.MaxBytes {
		return nil, time.Time{}, fmt.Errorf("%w: byte budget %d exhausted", ErrHotfixBudget, state.budget.MaxBytes)
	}
	return raw, time.Now().UTC(), nil
}

func wagoPageURL(base, search string, page int) (string, error) {
	parsed, err := url.Parse(base + "/hotfixes")
	if err != nil {
		return "", fmt.Errorf("%w: %v", ErrHotfixQuery, err)
	}
	values := parsed.Query()
	values.Set("page", strconv.Itoa(page))
	if search != "" {
		values.Set("search", search)
	}
	parsed.RawQuery = values.Encode()
	return parsed.String(), nil
}

// wagoEffectiveSearch preserves the legacy server-side candidate narrowing: an
// explicit search wins, then the record id, the push id and finally the build
// number. Exact filters are always applied locally afterwards.
func wagoEffectiveSearch(q HotfixQuery) string {
	if search := strings.TrimSpace(q.Search); search != "" {
		return search
	}
	if q.RecordID != nil {
		return strconv.FormatUint(uint64(*q.RecordID), 10)
	}
	if q.PushID != nil {
		return strconv.FormatInt(int64(*q.PushID), 10)
	}
	parts := strings.Split(q.FullBuild, ".")
	return parts[len(parts)-1]
}

func hotfixRecordFromWago(record WagoRecord) HotfixRecord {
	region := record.RegionID
	return HotfixRecord{
		Provider:      wagoProviderName,
		ID:            record.ID,
		Push:          record.PushID,
		RecordID:      record.RecordID,
		RegionID:      &region,
		TableName:     record.TableName,
		Status:        record.Status,
		Build:         record.Build,
		Locale:        record.Locale,
		CreatedAt:     record.CreatedAt,
		PayloadLength: len(record.Data),
		Payload:       record.Data,
	}
}

// applyWagoDecode preserves decodeState semantics: no_payload without payload
// bytes, not_valid when the raw status is not a valid row, decoded otherwise.
func applyWagoDecode(records []HotfixRecord, definition schema.Definition) error {
	for i := range records {
		record := &records[i]
		record.DecodeState = "no_payload"
		if len(record.Payload) == 0 {
			continue
		}
		if record.Status != 1 {
			record.DecodeState = "not_valid"
			continue
		}
		fields, err := DecodeHotfixPositional(record.Payload, definition)
		if err != nil {
			return fmt.Errorf("record id %d: %w", record.ID, err)
		}
		record.Fields = fields
		record.DecodeState = "decoded"
	}
	return nil
}

// QueryWagoHotfix runs the wago use case: optional documented fallback
// composition, a pinned ChangePin-derived set and an archived result capture.
func QueryWagoHotfix(ctx context.Context, root string, request WagoHotfixRequest) (RemoteHotfixResult, error) {
	provider := &WagoHotfix{
		Root:     root,
		BaseURL:  request.BaseURL,
		Client:   request.Client,
		Offline:  request.Offline,
		MaxPages: request.MaxPages,
		Budget:   request.Budget,
	}
	var run HotfixProvider = provider
	if request.Fallback != nil {
		run = HotfixFallback{Primary: provider, Fallback: *request.Fallback}
	}
	result, err := run.QueryHotfix(ctx, request.Query)
	if err != nil {
		return RemoteHotfixResult{}, err
	}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (RemoteHotfixResult, error) {
		pinned, err := pinHotfixChange(ctx, selection.OpenPinner(metadata), request.Parent, result.Change)
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		result.Snapshot = pinned.ID
		body, err := json.Marshal(result)
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{
			Reader:    bytes.NewReader(body),
			MaxBytes:  32 << 20,
			MediaType: "application/json",
			Complete:  result.Complete,
			Truncated: result.Truncated,
			Provenance: evidence.Provenance{
				Kind:      "hotfix-records",
				Locator:   result.Change.Provider + ":sha256:" + result.Change.SHA256,
				Snapshot:  pinned.ID,
				DataBuild: result.Query.FullBuild,
			},
		})
		if err != nil {
			return RemoteHotfixResult{}, err
		}
		result.Capture = &capture
		return result, nil
	})
}
