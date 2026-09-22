package records_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/schema"
)

func writeHotfixSnapshotFile(t *testing.T, version uint32, fixtures ...cacheRecordFixture) (string, []byte) {
	t.Helper()
	raw := makeCacheSnapshot(version, 68974, version, fixtures...)
	path := filepath.Join(t.TempDir(), "DBCache")
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	return path, raw
}

func snapshotTestQuery() records.HotfixQuery {
	query := wagoTestQuery()
	query.TableHash = records.HotfixUint32(0x11223344)
	return query
}

func snapshotDefinition(t *testing.T, text string) schema.Definition {
	t.Helper()
	doc, err := schema.Parse(context.Background(), []byte(text))
	if err != nil {
		t.Fatal(err)
	}
	definition, err := doc.Select(context.Background(), "12.0.7.68974", "")
	if err != nil {
		t.Fatal(err)
	}
	return definition
}

func TestSnapshotHotfixQueriesExplicitFile(t *testing.T) {
	path, raw := writeHotfixSnapshotFile(t, 9,
		cacheRecordFixture{Region: 71, UniqueID: 7, Push: -3, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{0xde, 0xad}},
		cacheRecordFixture{Region: 71, UniqueID: 8, Push: 4, Table: 0x55667788, Record: 56, Status: 2, Payload: nil},
	)
	source, err := records.DBCacheSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	result, err := source.QueryHotfix(context.Background(), snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 1 || result.Records[0].Provider != "dbcache" || result.Records[0].Push != -3 {
		t.Fatalf("records = %+v", result.Records)
	}
	record := result.Records[0]
	if record.TableHash == nil || *record.TableHash != 0x11223344 || record.RecordID != 55 || record.Status != 1 {
		t.Fatalf("record identity = %+v", record)
	}
	if record.PayloadHex != "dead" || record.PayloadLength != 2 || record.Index == nil {
		t.Fatalf("record payload = %+v", record)
	}
	if record.RegionID == nil || *record.RegionID != 71 || record.UniqueID == nil || *record.UniqueID != 7 {
		t.Fatalf("record region/unique = %+v", record)
	}
	sum := sha256.Sum256(raw)
	digest := hex.EncodeToString(sum[:])
	if result.Coverage.Ref != digest || !result.Coverage.Complete || result.Coverage.Provider != "dbcache" {
		t.Fatalf("coverage = %+v", result.Coverage)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if result.Coverage.CapturedAt == nil || !result.Coverage.CapturedAt.Equal(info.ModTime().UTC()) {
		t.Fatalf("capturedAt = %v want %v", result.Coverage.CapturedAt, info.ModTime().UTC())
	}
	if result.Change.Provider != "dbcache" || result.Change.SHA256 != digest || result.Change.Region != "us" {
		t.Fatalf("change = %+v", result.Change)
	}
	if !result.Complete || result.Truncated {
		t.Fatalf("completeness = %v %v", result.Complete, result.Truncated)
	}
}

func TestRaidbotsSnapshotStaleRefusal(t *testing.T) {
	path, _ := writeHotfixSnapshotFile(t, 9,
		cacheRecordFixture{Region: 71, Push: 1, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
	)
	old := time.Now().Add(-31 * 24 * time.Hour)
	if err := os.Chtimes(path, old, old); err != nil {
		t.Fatal(err)
	}
	raidbots, err := records.RaidbotsSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	_, err = raidbots.QueryHotfix(context.Background(), snapshotTestQuery())
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_snapshot_stale") {
		t.Fatalf("error = %v, want records.hotfix_snapshot_stale", err)
	}
	// The same stale file is fine for the live cache source, which has no
	// staleness policy.
	dbcache, err := records.DBCacheSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	if _, err := dbcache.QueryHotfix(context.Background(), snapshotTestQuery()); err != nil {
		t.Fatalf("dbcache source refused stale file: %v", err)
	}
	// Fresh enough snapshots pass the 30 day policy.
	fresh := time.Now().Add(-29 * 24 * time.Hour)
	if err := os.Chtimes(path, fresh, fresh); err != nil {
		t.Fatal(err)
	}
	result, err := raidbots.QueryHotfix(context.Background(), snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	if result.Coverage.Provider != "raidbots" || result.Coverage.Complete {
		t.Fatalf("raidbots coverage = %+v", result.Coverage)
	}
}

func TestSnapshotHotfixIdentityAndBuildChecks(t *testing.T) {
	path, _ := writeHotfixSnapshotFile(t, 9,
		cacheRecordFixture{Region: 71, Push: 1, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
	)
	source, err := records.DBCacheSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	mismatched := snapshotTestQuery()
	mismatched.Locale = "zhCN"
	mismatched.Region = "cn"
	_, err = source.QueryHotfix(context.Background(), mismatched)
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_identity") {
		t.Fatalf("context mismatch error = %v", err)
	}
	wrongBuild := filepath.Join(t.TempDir(), "DBCache")
	if err := os.WriteFile(wrongBuild, makeCacheSnapshot(9, 12345, 9, cacheRecordFixture{Region: 71, Push: 1, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}}), 0o600); err != nil {
		t.Fatal(err)
	}
	other, err := records.DBCacheSnapshot(wrongBuild, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	if _, err := other.QueryHotfix(context.Background(), snapshotTestQuery()); err == nil || !strings.Contains(err.Error(), "records.hotfix_build_mismatch") {
		t.Fatalf("build mismatch error = %v", err)
	}
}

func TestSnapshotHotfixFilterEdges(t *testing.T) {
	path, _ := writeHotfixSnapshotFile(t, 8,
		cacheRecordFixture{Push: 1, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
	)
	source, err := records.DBCacheSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	cases := map[string]func(records.HotfixQuery) records.HotfixQuery{
		"search":   func(q records.HotfixQuery) records.HotfixQuery { q.Search = "55"; return q },
		"from":     func(q records.HotfixQuery) records.HotfixQuery { q.From = &time.Time{}; return q },
		"to":       func(q records.HotfixQuery) records.HotfixQuery { q.To = &time.Time{}; return q },
		"table":    func(q records.HotfixQuery) records.HotfixQuery { q.Table = "SpellEffect"; q.TableHash = nil; return q },
		"page":     func(q records.HotfixQuery) records.HotfixQuery { q.Page = 2; return q },
		"regionId": func(q records.HotfixQuery) records.HotfixQuery { q.RegionID = records.HotfixUint32(71); return q },
	}
	for name, mutate := range cases {
		_, err := source.QueryHotfix(context.Background(), mutate(snapshotTestQuery()))
		if err == nil || !strings.Contains(err.Error(), "records.hotfix_filter_unsupported") {
			t.Fatalf("%s: error = %v, want records.hotfix_filter_unsupported", name, err)
		}
	}
	// A version 9 file carries region ids and accepts the filter.
	v9, _ := writeHotfixSnapshotFile(t, 9,
		cacheRecordFixture{Region: 71, Push: 1, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
	)
	regional, err := records.DBCacheSnapshot(v9, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	query := snapshotTestQuery()
	query.RegionID = records.HotfixUint32(71)
	result, err := regional.QueryHotfix(context.Background(), query)
	if err != nil || len(result.Records) != 1 {
		t.Fatalf("region filter = %+v err = %v", result.Records, err)
	}
}

func TestSnapshotHotfixLatestBatchAndCursor(t *testing.T) {
	path, raw := writeHotfixSnapshotFile(t, 7,
		cacheRecordFixture{Push: 5, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
		cacheRecordFixture{Push: 5, Table: 0x11223344, Record: 56, Status: 1, Payload: []byte{2}},
		cacheRecordFixture{Push: 3, Table: 0x11223344, Record: 57, Status: 1, Payload: []byte{3}},
	)
	source, err := records.DBCacheSnapshot(path, snapshotTestQuery())
	if err != nil {
		t.Fatal(err)
	}
	latest := snapshotTestQuery()
	latest.Latest = true
	result, err := source.QueryHotfix(context.Background(), latest)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Records) != 2 || result.Coverage.SelectedPush == nil || *result.Coverage.SelectedPush != 5 {
		t.Fatalf("latest = %+v selected = %v", result.Records, result.Coverage.SelectedPush)
	}

	windowed := snapshotTestQuery()
	windowed.Limit = 1
	first, err := source.QueryHotfix(context.Background(), windowed)
	if err != nil {
		t.Fatal(err)
	}
	if !first.Truncated || first.NextCursor == "" || len(first.Records) != 1 {
		t.Fatalf("window = %+v", first.Coverage)
	}
	cursor, err := records.DecodeHotfixCursor(first.NextCursor)
	if err != nil {
		t.Fatal(err)
	}
	if cursor.Source != "dbcache" || cursor.AfterIndex == nil || *cursor.AfterIndex != 0 {
		t.Fatalf("cursor = %+v", cursor)
	}
	second := windowed
	second.Cursor = first.NextCursor
	next, err := source.QueryHotfix(context.Background(), second)
	if err != nil {
		t.Fatal(err)
	}
	if len(next.Records) != 1 || next.Records[0].RecordID == first.Records[0].RecordID {
		t.Fatalf("resumed window = %+v", next.Records)
	}

	// The cursor binds to immutable bytes: a rewritten snapshot is refused.
	if err := os.WriteFile(path, append(raw, 0), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err = source.QueryHotfix(context.Background(), second)
	if err == nil || !strings.Contains(err.Error(), "records.hotfix_cursor") {
		t.Fatalf("tampered snapshot error = %v", err)
	}
}

func TestSnapshotHotfixDecodeStates(t *testing.T) {
	definition := snapshotDefinition(t, "COLUMNS\nint ID\nint Value\n\nBUILD 12.0.7.68974\n$id$ID<u32>\nValue<32>\n")
	valid := append([]byte{55, 0, 0, 0}, 0xf9, 0xff, 0xff, 0xff) // ID 55, Value -7
	path, _ := writeHotfixSnapshotFile(t, 7,
		cacheRecordFixture{Push: 5, Table: 0x11223344, Record: 55, Status: 1, Payload: valid},
		cacheRecordFixture{Push: 5, Table: 0x11223344, Record: 56, Status: 2, Payload: valid},
		cacheRecordFixture{Push: 5, Table: 0x11223344, Record: 57, Status: 1, Payload: nil},
	)
	query := snapshotTestQuery()
	query.Definition = &definition
	query.RecordID = nil
	source, err := records.DBCacheSnapshot(path, query)
	if err != nil {
		t.Fatal(err)
	}
	result, err := source.QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	states := map[uint32]string{}
	for _, record := range result.Records {
		states[record.RecordID] = record.DecodeState
		if record.RecordID == 55 {
			if record.Fields["ID"] != uint64(55) || record.Fields["Value"] != int64(-7) {
				t.Fatalf("decoded fields = %+v", record.Fields)
			}
		}
	}
	if states[55] != "decoded" || states[56] != "not_valid" || states[57] != "no_payload" {
		t.Fatalf("decode states = %v", states)
	}
}

type stubHotfixProvider struct {
	result records.RemoteHotfixResult
	err    error
	calls  int
}

func (s *stubHotfixProvider) QueryHotfix(ctx context.Context, query records.HotfixQuery) (records.RemoteHotfixResult, error) {
	s.calls++
	return s.result, s.err
}

func stubResult(provider string, complete bool, ids ...uint32) records.RemoteHotfixResult {
	recordsOut := make([]records.HotfixRecord, 0, len(ids))
	for _, id := range ids {
		recordsOut = append(recordsOut, records.HotfixRecord{Provider: provider, Push: 1, RecordID: id, Status: 1, Build: "68974"})
	}
	return records.RemoteHotfixResult{
		Records:  recordsOut,
		Coverage: records.HotfixCoverage{Provider: provider, Complete: complete, Ref: strings.Repeat("a", 64), Returned: len(ids)},
		Complete: complete,
	}
}

func TestHotfixFallbackComposition(t *testing.T) {
	query := wagoTestQuery()

	// A complete primary never consults the fallback: no silent merge.
	primary := &stubHotfixProvider{result: stubResult("wago", true, 1)}
	fallback := &stubHotfixProvider{result: stubResult("raidbots", false, 2)}
	composed, err := (records.HotfixFallback{Primary: primary, Fallback: fallback}).QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if len(composed.Records) != 1 || fallback.calls != 0 {
		t.Fatalf("records = %d fallback calls = %d", len(composed.Records), fallback.calls)
	}

	// Incomplete primary coverage composes with the fallback and reports both.
	primary = &stubHotfixProvider{result: stubResult("wago", false, 1, 2)}
	fallback = &stubHotfixProvider{result: stubResult("raidbots", false, 2, 3)}
	composed, err = (records.HotfixFallback{Primary: primary, Fallback: fallback}).QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if len(composed.Records) != 4 {
		t.Fatalf("composed records = %+v", composed.Records)
	}
	if composed.Coverage.Provider != "wago+raidbots" || composed.Complete || len(composed.Sources) != 2 {
		t.Fatalf("combined coverage = %+v sources = %+v", composed.Coverage, composed.Sources)
	}
	for index, record := range composed.Records {
		want := "wago"
		if index >= 2 {
			want = "raidbots"
		}
		if record.Provider != want {
			t.Fatalf("record %d provenance = %+v", index, record)
		}
	}
	if !containsWarning(composed.Warnings, "records.hotfix_fallback_used") ||
		!containsWarning(composed.Warnings, "records.hotfix_multi_source") ||
		!containsWarning(composed.Warnings, "records.hotfix_cross_source_duplicate:1") {
		t.Fatalf("warnings = %v", composed.Warnings)
	}

	// A failed primary falls back with the reason reported.
	primary = &stubHotfixProvider{err: errors.New("records.hotfix_wago_http: boom")}
	fallback = &stubHotfixProvider{result: stubResult("raidbots", false, 3)}
	composed, err = (records.HotfixFallback{Primary: primary, Fallback: fallback}).QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	if len(composed.Records) != 1 || composed.Coverage.Provider != "raidbots" {
		t.Fatalf("fallback result = %+v", composed.Coverage)
	}
	reasoned := false
	for _, warning := range composed.Warnings {
		if strings.HasPrefix(warning, "records.hotfix_fallback_reason:") {
			reasoned = true
		}
	}
	if !reasoned {
		t.Fatalf("warnings = %v", composed.Warnings)
	}

	// Both sources failing keeps the primary error visible.
	primary = &stubHotfixProvider{err: errors.New("records.hotfix_wago_http: boom")}
	fallback = &stubHotfixProvider{err: errors.New("records.hotfix_snapshot_stale: boom")}
	if _, err = (records.HotfixFallback{Primary: primary, Fallback: fallback}).QueryHotfix(context.Background(), query); err == nil || !strings.Contains(err.Error(), "records.hotfix_wago_http") {
		t.Fatalf("error = %v", err)
	}

	// Cursor-bound queries stay with their own source.
	cursor, err := records.EncodeHotfixCursor(records.HotfixCursor{Source: "wago", Ref: strings.Repeat("b", 64), Scope: "x", ResumePage: 2})
	if err != nil {
		t.Fatal(err)
	}
	primary = &stubHotfixProvider{err: errors.New("records.hotfix_wago_http: boom")}
	fallback = &stubHotfixProvider{result: stubResult("raidbots", false, 3)}
	cursorQuery := query
	cursorQuery.Cursor = cursor
	if _, err = (records.HotfixFallback{Primary: primary, Fallback: fallback}).QueryHotfix(context.Background(), cursorQuery); err == nil {
		t.Fatal("cursor query silently fell back")
	}
	if fallback.calls != 0 {
		t.Fatalf("fallback calls = %d", fallback.calls)
	}
}

func TestHotfixFallbackRealProvidersCompose(t *testing.T) {
	pages := map[int][]byte{
		1: wagoFixturePage(1, 2, 2, wagoRow(11, 100, 55, 1, "SpellEffect", `[1]`, "2026-08-05 22:09:07", 71, "enUS")),
		2: wagoFixturePage(2, 2, 2, wagoRow(12, 100, 56, 1, "SpellEffect", `[2]`, "2026-08-05 22:09:07", 71, "enUS")),
	}
	server := newWagoTestServer(t, pages)
	wago := &records.WagoHotfix{Root: hotfixTestWorkspace(t), BaseURL: server.URL, Client: server.Client(), MaxPages: 1}
	path, _ := writeHotfixSnapshotFile(t, 9,
		cacheRecordFixture{Region: 71, Push: 7, Table: 0x11223344, Record: 55, Status: 1, Payload: []byte{1}},
	)
	snapshotQuery := snapshotTestQuery()
	snapshotQuery.TableHash = nil
	snapshot, err := records.DBCacheSnapshot(path, snapshotQuery)
	if err != nil {
		t.Fatal(err)
	}
	query := wagoTestQuery()
	composed, err := (records.HotfixFallback{Primary: wago, Fallback: snapshot}).QueryHotfix(context.Background(), query)
	if err != nil {
		t.Fatal(err)
	}
	providers := map[string]bool{}
	for _, record := range composed.Records {
		providers[record.Provider] = true
	}
	if !providers["wago"] || !providers["dbcache"] || len(composed.Records) < 2 {
		t.Fatalf("composed records = %+v", composed.Records)
	}
	if composed.Coverage.Provider != "wago+dbcache" || composed.Complete {
		t.Fatalf("combined coverage = %+v", composed.Coverage)
	}
	if len(composed.Sources) != 2 || !containsWarning(composed.Warnings, "records.hotfix_multi_source") {
		t.Fatalf("sources = %+v warnings = %v", composed.Sources, composed.Warnings)
	}
}
