package records_test

import (
	"context"
	"fmt"
	"html"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

func wagoRow(id, push, record int64, status int, table, data, createdAt string, region int64, locale string) string {
	return fmt.Sprintf(`{"id":%d,"push_id":%d,"record_id":%d,"status":%d,"build":68974,"table_name":%q,"data":%s,"created_at":%q,"region_id":%d,"locale":%q,"search_text":""}`,
		id, push, record, status, table, data, createdAt, region, locale)
}

func wagoFixturePageComponent(component string, current, last, total int, records string) []byte {
	next := "null"
	if current < last {
		next = fmt.Sprintf("%q", "/hotfixes?page="+strconv.Itoa(current+1))
	}
	payload := fmt.Sprintf(`{"component":%q,"props":{"hotfixes":{"current_page":%d,"last_page":%d,"per_page":25,"total":%d,"data":[%s],"next_page_url":%s}}}`,
		component, current, last, total, records, next)
	return []byte(`<html><body><div id="app" data-page="` + html.EscapeString(payload) + `"></div></body></html>`)
}

func wagoFixturePage(current, last, total int, records string) []byte {
	return wagoFixturePageComponent("Hotfixes", current, last, total, records)
}

type wagoTestServer struct {
	*httptest.Server
	calls atomic.Int32
	pages map[int][]byte
	seen  map[int]string
	mu    chan struct{}
}

func newWagoTestServer(t *testing.T, pages map[int][]byte) *wagoTestServer {
	t.Helper()
	server := &wagoTestServer{pages: pages, seen: map[int]string{}, mu: make(chan struct{}, 1)}
	server.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		server.calls.Add(1)
		page, _ := strconv.Atoi(r.URL.Query().Get("page"))
		if page == 0 {
			page = 1
		}
		server.seen[page] = r.URL.Query().Get("search")
		raw, ok := server.pages[page]
		if !ok {
			http.Error(w, "missing page", http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "text/html")
		_, _ = w.Write(raw)
	}))
	t.Cleanup(server.Close)
	return server
}

func hotfixTestWorkspace(t *testing.T) string {
	t.Helper()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatalf("vault.Initialize: %v", err)
	}
	return root
}

func wagoTestQuery() records.HotfixQuery {
	return records.HotfixQuery{Product: "retail", FullBuild: "12.0.7.68974", Region: "us", Locale: "enUS", Limit: 50}
}

func TestWagoHotfixMultiPageWithIdentityKeys(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 2, 4,
			wagoRow(11, 100, 55, 1, "SpellEffect", `[1,2]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
				wagoRow(12, 100, 56, 1, "SpellEffect", `[3,4]`, "2026-08-05 22:09:08", 71, "enUS")),
		2: wagoFixturePage(2, 2, 4,
			wagoRow(13, 99, 57, 3, "SpellEffect", `null`, "2026-08-05 22:09:09", 71, "enUS")+", "+
				wagoRow(14, 99, 58, 3, "SpellEffect", `null`, "2026-08-05 22:09:10", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	root := hotfixTestWorkspace(t)
	provider := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client()}
	query := wagoTestQuery()
	query.Table = "SpellEffect"
	result, err := provider.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 4 || !result.Complete || result.Truncated {
		t.Fatalf("records = %d complete = %v truncated = %v", len(result.Records), result.Complete, result.Truncated)
	}
	if result.Coverage.StartPage != 1 || result.Coverage.EndPage != 2 || result.Coverage.LastPage != 2 || result.Coverage.Total != 4 {
		t.Fatalf("coverage = %+v", result.Coverage)
	}
	if result.Coverage.Scanned != 4 || result.Coverage.Matched != 4 || result.Coverage.Returned != 4 {
		t.Fatalf("coverage counts = %+v", result.Coverage)
	}
	for _, record := range result.Records {
		if record.Provider != "wago" || record.TableName != "SpellEffect" {
			t.Fatalf("record provenance = %+v", record)
		}
	}
	if result.Change.Provider != "wago" || result.Change.Product != "retail" || result.Change.FullBuild != "12.0.7.68974" || result.Change.Region != "us" {
		t.Fatalf("change pin = %+v", result.Change)
	}
	if len(result.Change.SHA256) != 64 || result.Change.FetchedAt.IsZero() || !strings.HasPrefix(result.Change.Scope, "wago:product:retail") {
		t.Fatalf("change pin identity = %+v", result.Change)
	}
	if server.seen[1] != "68974" || server.seen[2] != "68974" {
		t.Fatalf("default search identity = %v", server.seen)
	}

	// Repeated queries reuse the verified per-page receipts: no second fetch.
	calls := server.calls.Load()
	repeat, err := provider.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if server.calls.Load() != calls {
		t.Fatalf("cache miss refetched: %d -> %d", calls, server.calls.Load())
	}
	if len(repeat.Records) != 4 || repeat.Change.SHA256 != result.Change.SHA256 {
		t.Fatalf("repeat result = %+v", repeat.Coverage)
	}
	if !containsWarning(repeat.Warnings, "records.wago_cache_hit") {
		t.Fatalf("warnings = %v", repeat.Warnings)
	}
}

func containsWarning(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}

func TestWagoHotfixPageDriftFails(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 2, 4, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
		2: wagoFixturePage(2, 2, 5, wagoRow(12, 100, 56, 1, "SpellEffect", `[2]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	provider := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client()}
	_, err := provider.QueryHotfix(context.Background(), wagoTestQuery())
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_page_drift") {
		t.Fatalf("error = %v, want records.hotfix_page_drift", err)
	}
}

func TestWagoHotfixMaxPagesBoundIsExplicit(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 3, 3, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
		2: wagoFixturePage(2, 3, 3, wagoRow(12, 99, 56, 1, "SpellEffect", `[2]`, "2026-08-05 22:09:07", 71, "enUS")),
		3: wagoFixturePage(3, 3, 3, wagoRow(13, 98, 57, 1, "SpellEffect", `[3]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	root := hotfixTestWorkspace(t)
	provider := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client(), MaxPages: 2}

	latest := wagoTestQuery()
	latest.Latest = true
	if _, err := provider.QueryHotfix(context.Background(), latest); err == nil || !strings.Contains(err.Error(), "records.hotfix_budget") {
		t.Fatalf("latest over bound error = %v, want records.hotfix_budget", err)
	}

	// A windowed walk stops at the visible page budget with an explicit
	// truncation reason and a page-identity cursor, never a silent partial.
	windowed := wagoTestQuery()
	result, err := provider.QueryHotfix(context.Background(), windowed)
	if err != nil {
		t.Fatal(err)
	}
	if !result.Truncated || result.Coverage.TruncationReason != "page_budget" || result.Complete {
		t.Fatalf("result = %+v", result.Coverage)
	}
	if result.NextCursor == "" || len(result.Records) != 2 {
		t.Fatalf("records = %d cursor = %q", len(result.Records), result.NextCursor)
	}

	// Resuming continues at the next page with drift validation.
	resumed := wagoTestQuery()
	resumed.Cursor = result.NextCursor
	next, err := provider.QueryHotfix(context.Background(), resumed)
	if err != nil {
		t.Fatal(err)
	}
	if len(next.Records) != 1 || next.Records[0].RecordID != 57 {
		t.Fatalf("resumed records = %+v", next.Records)
	}
	if !next.Complete {
		t.Fatalf("resumed coverage = %+v", next.Coverage)
	}
}

func TestWagoHotfixCorruptCacheFailsExplicitly(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 1, 1, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	root := hotfixTestWorkspace(t)
	provider := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client()}
	if _, err := provider.QueryHotfix(context.Background(), wagoTestQuery()); err != nil {
		t.Fatal(err)
	}
	calls := server.calls.Load()
	blobs, err := filepath.Glob(filepath.Join(root, "blobs", "*", "*"))
	if err != nil || len(blobs) == 0 {
		t.Fatalf("blobs = %v err = %v", blobs, err)
	}
	if err := os.WriteFile(blobs[0], []byte("corrupt"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err = provider.QueryHotfix(context.Background(), wagoTestQuery())
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_cache_corrupt") {
		t.Fatalf("error = %v, want records.hotfix_cache_corrupt", err)
	}
	if server.calls.Load() != calls {
		t.Fatalf("corrupt cache silently refetched: %d -> %d", calls, server.calls.Load())
	}
}

func TestWagoHotfixOfflineRefusesNetwork(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 1, 1, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	root := hotfixTestWorkspace(t)
	offline := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client(), Offline: true}
	if _, err := offline.QueryHotfix(context.Background(), wagoTestQuery()); err == nil || !strings.Contains(err.Error(), "records.hotfix_offline") {
		t.Fatalf("error = %v, want records.hotfix_offline", err)
	}
	if server.calls.Load() != 0 {
		t.Fatalf("offline query touched the network: %d", server.calls.Load())
	}
	online := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client()}
	if _, err := online.QueryHotfix(context.Background(), wagoTestQuery()); err != nil {
		t.Fatal(err)
	}
	calls := server.calls.Load()
	result, err := offline.QueryHotfix(context.Background(), wagoTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	if server.calls.Load() != calls || len(result.Records) != 1 {
		t.Fatalf("offline cache read fetched %d extra calls, records = %d", server.calls.Load()-calls, len(result.Records))
	}
}

func TestWagoHotfixLatestBatchPerProvider(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 2, 3,
			wagoRow(11, 5, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
				wagoRow(12, 3, 56, 1, "SpellEffect", `[2]`, "2026-08-05 22:09:07", 71, "enUS")),
		2: wagoFixturePage(2, 2, 3, wagoRow(13, 5, 57, 1, "SpellEffect", `[3]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	provider := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client()}
	latest := wagoTestQuery()
	latest.Latest = true
	result, err := provider.QueryHotfix(context.Background(), latest)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 2 || result.Coverage.SelectedPush == nil || *result.Coverage.SelectedPush != 5 {
		t.Fatalf("latest batch = %+v selected = %v", result.Records, result.Coverage.SelectedPush)
	}
	for _, record := range result.Records {
		if record.Push != 5 || record.RecordID == 0 {
			t.Fatalf("batch record = %+v", record)
		}
	}

	// The batch is chosen before pagination and every same-batch record stays
	// reachable through the page-identity cursor.
	windowed := latest
	windowed.Limit = 1
	first, err := provider.QueryHotfix(context.Background(), windowed)
	if err != nil {
		t.Fatal(err)
	}
	if !first.Truncated || first.NextCursor == "" || len(first.Records) != 1 {
		t.Fatalf("window = %+v cursor = %q", first.Coverage, first.NextCursor)
	}
	second := windowed
	second.Cursor = first.NextCursor
	rest, err := provider.QueryHotfix(context.Background(), second)
	if err != nil {
		t.Fatal(err)
	}
	if len(rest.Records) != 1 || rest.Records[0].RecordID == first.Records[0].RecordID {
		t.Fatalf("resumed batch record = %+v", rest.Records)
	}
}

func TestWagoHotfixFilterEdges(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 1, 5,
			wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
				wagoRow(12, 99, 56, 3, "SpellEffect", "null", "2026-08-07 10:00:00", 196, "zhCN")+", "+
				wagoRow(13, 98, 57, 5, "SpellEffect", `[3]`, "2026-08-07 11:00:00", 71, "enUS")+", "+
				wagoRow(14, 97, 58, 1, "SpellEffect", `[4]`, "2026-08-08 09:00:00", 71, "enUS")+", "+
				wagoRow(15, 96, 59, 3, "SpellEffect", "null", "2026-08-08 10:00:00", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	provider := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client()}

	status := wagoTestQuery()
	status.Status = records.HotfixUint8(3)
	result, err := provider.QueryHotfix(context.Background(), status)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 1 || result.Records[0].Status != 3 || result.Records[0].RecordID != 59 {
		t.Fatalf("status filter = %+v", result.Records)
	}

	region := wagoTestQuery()
	region.Region = "cn"
	region.Locale = "zhCN"
	region.RegionID = records.HotfixUint32(196)
	result, err = provider.QueryHotfix(context.Background(), region)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 1 || *result.Records[0].RegionID != 196 {
		t.Fatalf("region filter = %+v", result.Records)
	}

	fromRFC := wagoTestQuery()
	from, err := records.ParseHotfixTime("2026-08-06T00:00:00Z")
	if err != nil {
		t.Fatal(err)
	}
	fromRFC.From = &from
	result, err = provider.QueryHotfix(context.Background(), fromRFC)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 3 {
		t.Fatalf("RFC3339 time filter = %+v", result.Records)
	}

	toNative := wagoTestQuery()
	to, err := records.ParseHotfixTime("2026-08-06 00:00:00")
	if err != nil {
		t.Fatal(err)
	}
	toNative.To = &to
	result, err = provider.QueryHotfix(context.Background(), toNative)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 1 || result.Records[0].RecordID != 55 {
		t.Fatalf("native time filter = %+v", result.Records)
	}

	search := wagoTestQuery()
	search.Search = "custom-search"
	if _, err := provider.QueryHotfix(context.Background(), search); err != nil {
		t.Fatal(err)
	}
	if server.seen[1] != "custom-search" {
		t.Fatalf("search identity = %q", server.seen[1])
	}

	miss := wagoTestQuery()
	miss.Table = "NoSuchTable"
	result, err = provider.QueryHotfix(context.Background(), miss)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 0 || !containsWarning(result.Warnings, "records.wago_search_miss") {
		t.Fatalf("search miss = %+v warnings = %v", result.Coverage, result.Warnings)
	}

	unknown := wagoTestQuery()
	result, err = provider.QueryHotfix(context.Background(), unknown)
	if err != nil {
		t.Fatal(err)
	}
	if !containsWarning(result.Warnings, "records.hotfix_unknown_status:5") {
		t.Fatalf("unknown status warnings = %v", result.Warnings)
	}
}

func TestWagoHotfixCursorRefusesShiftedSource(t *testing.T) {
	pageOne := wagoFixturePage(1, 1, 3,
		wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
			wagoRow(12, 100, 56, 1, "SpellEffect", `[2]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
			wagoRow(13, 100, 57, 1, "SpellEffect", `[3]`, "2026-08-05 22:09:07", 71, "enUS"))
	server := newWagoTestServer(t, map[int][]byte{1: pageOne})
	root := hotfixTestWorkspace(t)
	provider := &records.WagoHotfix{Root: root, BaseURL: server.URL, Client: server.Client()}
	query := wagoTestQuery()
	query.Limit = 2
	first, err := provider.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if !first.Truncated || first.NextCursor == "" {
		t.Fatalf("first window = %+v", first.Coverage)
	}

	// Happy path: the cached page keeps the resumed window immutable.
	resumed := query
	resumed.Cursor = first.NextCursor
	next, err := provider.QueryHotfix(context.Background(), resumed)
	if err != nil {
		t.Fatal(err)
	}
	if len(next.Records) != 1 || next.Records[0].RecordID != 57 {
		t.Fatalf("resumed window = %+v", next.Records)
	}

	// A shifted remote page cannot satisfy the cursor's page identity.
	shifted := wagoFixturePage(1, 1, 3,
		wagoRow(21, 100, 55, 1, "SpellEffect", `[9]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
			wagoRow(22, 100, 56, 1, "SpellEffect", `[9]`, "2026-08-05 22:09:07", 71, "enUS")+", "+
			wagoRow(23, 100, 57, 1, "SpellEffect", `[9]`, "2026-08-05 22:09:07", 71, "enUS"))
	server.pages[1] = shifted
	fresh := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client()}
	_, err = fresh.QueryHotfix(context.Background(), resumed)
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_page_drift") {
		t.Fatalf("error = %v, want records.hotfix_page_drift", err)
	}
}

func TestWagoHotfixCursorScopeMismatch(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 1, 1, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	provider := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client()}
	query := wagoTestQuery()
	query.Limit = 1
	query.Table = "SpellEffect"
	result, err := provider.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	// Complete single record: no cursor. Build one artificially to prove scope binding.
	cursor, err := records.EncodeHotfixCursor(records.HotfixCursor{Source: "wago", Ref: strings.Repeat("a", 64), Scope: "other", ResumePage: 1})
	if err != nil {
		t.Fatal(err)
	}
	mismatched := wagoTestQuery()
	mismatched.Limit = 1
	mismatched.Table = "SpellEffect"
	mismatched.Cursor = cursor
	_, err = provider.QueryHotfix(context.Background(), mismatched)
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_cursor") {
		t.Fatalf("error = %v, want records.hotfix_cursor (result %v)", err, result.Coverage)
	}
}

func TestQueryWagoHotfixPinsChangeAndCapture(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 1, 1, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	root := hotfixTestWorkspace(t)
	result, err := records.QueryWagoHotfix(context.Background(), root, records.WagoHotfixRequest{Query: wagoTestQuery(), BaseURL: server.URL, Client: server.Client()})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(result.Snapshot, "PIN-") {
		t.Fatalf("snapshot = %q", result.Snapshot)
	}
	if result.Capture == nil || !strings.HasPrefix(result.Capture.ID, "CAP-") || !result.Capture.Complete {
		t.Fatalf("capture = %+v", result.Capture)
	}
	if result.Capture.Provenance.Kind != "hotfix-records" || !strings.HasPrefix(result.Capture.Provenance.Locator, "wago:sha256:") {
		t.Fatalf("provenance = %+v", result.Capture.Provenance)
	}
}

func TestWagoHotfixRealFixtureQuery(t *testing.T) {
	raw := readHotfixFixture(t, "response-page-1.html")
	server := newWagoTestServer(t, map[int][]byte{1: raw})
	provider := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client(), MaxPages: 1}
	query := wagoTestQuery()
	query.Table = "SpellEffect"
	query.Search = "68974"
	query.RegionID = records.HotfixUint32(71)
	result, err := provider.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	// Legacy corpus oracle: the page yields at least one exact match.
	if len(result.Records) < 1 {
		t.Fatalf("fixture matches = %d coverage = %+v", len(result.Records), result.Coverage)
	}
	if result.Coverage.LastPage != 680096 || result.Coverage.Total != 17002386 {
		t.Fatalf("fixture pagination = %+v", result.Coverage)
	}
	if !result.Truncated || result.Coverage.TruncationReason != "page_budget" {
		t.Fatalf("single page window = %+v", result.Coverage)
	}
}
