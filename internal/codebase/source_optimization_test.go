package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestSourceRequestSharesVerificationDocumentsAndBatch(t *testing.T) {
	var declarations strings.Builder
	for i := 0; i < 40; i++ {
		declarations.WriteString("function Widget() end\n")
	}
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": declarations.String(), "b.lua": "function Other() end\n"})
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	if err := b.EnsureIndex(ctx, pin); err != nil {
		t.Fatal(err)
	}
	result, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "Widget", Limit: 100})
	if err != nil || len(result.Results) != 40 {
		t.Fatalf("results=%d err=%v", len(result.Results), err)
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if cache.work.VerificationPasses != 1 || cache.work.DocumentReads != 1 || cache.work.GitBatchStarts != 1 {
		t.Fatalf("repeated work: %+v", cache.work)
	}
	if _, _, err := cache.document(ctx, "b.lua"); err != nil {
		t.Fatal(err)
	}
	if cache.work.DocumentReads != 2 || cache.work.GitBatchStarts != 1 {
		t.Fatalf("cross-file batch not reused: %+v", cache.work)
	}
	t.Logf("40 results, one request: %+v", cache.work)
	closeQuery()
	if cache.records != nil || cache.batch != nil {
		t.Fatal("request resources leaked")
	}
}

func TestDerivedOffsetsMatchLegacySearchAndResearchPages(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if cache.offsets == nil {
		t.Fatal("sealed offsets absent")
	}
	for _, query := range []SearchQuery{{Text: "Widget", Limit: 1}, {Text: "Widget", Mode: SearchModeExploratory, Limit: 2}, {Text: "widget factory", Mode: SearchModeExploratory, Topic: "lua"}, {Text: "Interface", Topic: "toc"}, {Text: "WIDGET_READY"}} {
		want, err := b.Search(ctx, "PIN", pin, query)
		if err != nil {
			t.Fatal(err)
		}
		offsets := cache.offsets
		cache.offsets = nil
		got, err := b.Search(ctx, "PIN", pin, query)
		cache.offsets = offsets
		if err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("query=%+v indexed=%+v legacy=%+v err=%v", query, want, got, err)
		}
		if want.NextCursor != "" {
			query.Cursor = want.NextCursor
			want, err = b.Search(ctx, "PIN", pin, query)
			if err != nil {
				t.Fatal(err)
			}
			cache.offsets = nil
			got, err = b.Search(ctx, "PIN", pin, query)
			cache.offsets = offsets
			if err != nil || !reflect.DeepEqual(got, want) {
				t.Fatal("search cursor changed")
			}
		}
	}
	for _, name := range []string{"WidgetFactory", "Widget", "WIDGET_READY"} {
		query := RelationQuery{Symbol: name, Limit: 1}
		want, err := b.Relate(ctx, "PIN", pin, query, ResearchOptions{})
		if err != nil {
			t.Fatal(err)
		}
		offsets := cache.offsets
		cache.offsets = nil
		got, err := b.Relate(ctx, "PIN", pin, query, ResearchOptions{})
		cache.offsets = offsets
		got.Coverage.ScannedFacts = want.Coverage.ScannedFacts
		if err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("relation changed: %s indexed=%+v legacy=%+v err=%v", name, want, got, err)
		}
		contextQuery := ContextQuery{Symbol: name, Depth: 3, Limit: 1, MaxBytes: 32}
		contextWant, err := b.Context(ctx, "PIN", pin, contextQuery, ResearchOptions{})
		if err != nil {
			t.Fatal(err)
		}
		cache.offsets = nil
		contextGot, err := b.Context(ctx, "PIN", pin, contextQuery, ResearchOptions{})
		cache.offsets = offsets
		contextGot.Coverage.ScannedFacts = contextWant.Coverage.ScannedFacts
		if err != nil || !reflect.DeepEqual(contextGot, contextWant) {
			t.Fatalf("context changed: %s err=%v", name, err)
		}
	}
}

func TestDerivedOffsetsDetectCorruptionAndRebuildMissing(t *testing.T) {
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": "function Handler() end\n"})
	file := filepath.Join(b.indexPath(pin), "offsets.json")
	raw, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(file, append(raw, ' '), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err = b.openIndex(context.Background(), pin); !errors.Is(err, ErrSourceIntegrity) || !strings.Contains(err.Error(), "content_mismatch") {
		t.Fatalf("corrupt sidecar accepted: %v", err)
	}
	if err = os.Remove(file); err != nil {
		t.Fatal(err)
	}
	cache, _, err := b.openIndex(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if cache.offsets == nil {
		t.Fatal("missing sidecar not rebuilt")
	}
	var got []sourceRecord
	if err = cache.scanSelected(context.Background(), func(e recordOffset) bool { return e.SymbolKind == "declaration" }, func(r sourceRecord) error { got = append(got, r); return nil }); err != nil || len(got) != 1 {
		t.Fatalf("rebuild rows=%d err=%v", len(got), err)
	}
	// The fixed handle must not authorize externally rewritten record bytes.
	recordsPath := filepath.Join(cache.dir, "records.jsonl")
	records, err := os.ReadFile(recordsPath)
	if err != nil {
		t.Fatal(err)
	}
	records = []byte(strings.Replace(string(records), "Handler", "Hacker_", -1))
	if err = os.WriteFile(recordsPath, records, 0600); err != nil {
		t.Fatal(err)
	}
	if err = cache.scanSelected(context.Background(), func(e recordOffset) bool { return e.SymbolKind == "declaration" }, func(sourceRecord) error { return nil }); err == nil || !strings.Contains(err.Error(), "content_mismatch") {
		t.Fatalf("rewrite accepted: %v", err)
	}
}

func TestSourceFreshRequestStillVerifiesAndCancellationIsBounded(t *testing.T) {
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": "function Widget() end\n"})
	if _, err := b.Search(context.Background(), "PIN", pin, SearchQuery{Text: "Widget"}); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(b.indexPath(pin), "records.jsonl")
	raw, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	raw[0] = ' '
	if err = os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := b.Search(context.Background(), "PIN", pin, SearchQuery{Text: "Widget"}); !errors.Is(err, ErrSourceIntegrity) {
		t.Fatal("new request trusted old verification")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, _, err = b.openIndex(ctx, pin); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation lost: %v", err)
	}
}

func TestBodyTopicFiltersBeforeOutputBudget(t *testing.T) {
	b, pin := indexedSearchFixture(t, map[string]string{
		"Unrelated.lua": strings.Repeat("-- BODYNEEDLE "+strings.Repeat("x", 1100)+"\n", 4000),
		"Relevant.XML":  "<!-- BODYNEEDLE -->\n",
	})
	result, err := b.Search(context.Background(), "PIN", pin, SearchQuery{Text: "BODYNEEDLE", Topic: "xml"})
	if err != nil || result.Truncated || len(result.Results) != 1 || result.Results[0].Path != "Relevant.XML" {
		t.Fatalf("irrelevant output consumed budget: %+v err=%v", result, err)
	}
}

func TestSourceCapacityCensusOnceAcrossGrowthAndCrashResidue(t *testing.T) {
	b, root := budgetBrowser(t)
	budgetFile(t, filepath.Join(root, "facts", "base.json"), 20)
	metrics := &sourceWalkMetrics{}
	ctx := context.WithValue(context.Background(), sourceWalkMetricsKey{}, metrics)
	lease, err := b.reserveSourceCapacityBudget(ctx, 10, "", 100)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 7; i++ {
		if err = lease.Grow(ctx, 10); err != nil {
			t.Fatal(err)
		}
	}
	if metrics.Walks != 1 {
		t.Fatalf("repeated full namespace walks: %+v", metrics)
	}
	budgetFile(t, filepath.Join(root, "facts", ".crashed-stage"), 80)
	if err = lease.Grow(ctx, 1); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("unwritten grant overcommit: %v", err)
	}
	lease.Close()
	if _, err = b.reserveSourceCapacityBudget(ctx, 1, "", 100); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("crash residue omitted: %v", err)
	}
	t.Logf("one census + 7 grows; next request counts stage residue: %+v", metrics)
}

func TestLegacyManifestRebuildsOffsetsWithoutDatabase(t *testing.T) {
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": "function Widget() end\n"})
	file := filepath.Join(b.indexPath(pin), "manifest.json")
	raw, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	var manifest cacheManifest
	if err = json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.DerivedSchema = ""
	manifest.DerivedHash = ""
	manifest.DerivedBytes = 0
	raw, _ = json.Marshal(manifest)
	if err = os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	cache, _, err := b.openIndex(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if cache.offsets == nil {
		t.Fatal("legacy file-cache not rebuilt")
	}
}

func TestDerivedDirectionalDecodeWork(t *testing.T) {
	var text strings.Builder
	text.WriteString("function Selected() end\n")
	for i := 0; i < 2000; i++ {
		fmt.Fprintf(&text, "function Unrelated%d() end\n", i)
	}
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": text.String()})
	cache, _, err := b.openIndex(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	var indexed, legacy []sourceRecord
	cache.work = sourceQueryWork{}
	if err = cache.scanSelected(context.Background(), func(e recordOffset) bool {
		return e.Kind == "symbol" && e.SymbolKind == "declaration" && e.Name == "Selected"
	}, func(r sourceRecord) error { indexed = append(indexed, r); return nil }); err != nil {
		t.Fatal(err)
	}
	indexedWork := cache.work
	cache.work = sourceQueryWork{}
	if err = cache.scanLegacy(context.Background(), func(r sourceRecord) error {
		if r.Kind == "symbol" && r.Symbol != nil && r.Symbol.Kind == "declaration" && r.Symbol.Name == "Selected" {
			legacy = append(legacy, r)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(indexed, legacy) || indexedWork.DecodedRecords != 1 || cache.work.DecodedRecords < 2000 {
		t.Fatalf("indexed=%+v legacy=%+v", indexedWork, cache.work)
	}
	t.Logf("same fixed declaration: derived JSON decodes=%d selected bytes=%d; legacy JSON decodes=%d scan bytes=%d; metadata visited=%d", indexedWork.DecodedRecords, indexedWork.RecordReadBytes, cache.work.DecodedRecords, cache.work.RecordReadBytes, indexedWork.MetadataVisits)
	t.Logf("sealed records bytes=%d offsets bytes=%d disk amplification=%.3f", cache.manifest.RecordBytes, cache.manifest.DerivedBytes, 1+float64(cache.manifest.DerivedBytes)/float64(cache.manifest.RecordBytes))
}

type cancelAfterRead struct {
	reader *bytes.Reader
	cancel context.CancelFunc
}

func (r *cancelAfterRead) Read(p []byte) (int, error) {
	n, err := r.reader.Read(p)
	r.cancel()
	return n, err
}

func TestSourceHashCancellationChecksEveryChunk(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	r := &cancelAfterRead{reader: bytes.NewReader(make([]byte, 128<<10)), cancel: cancel}
	n, err := io.CopyBuffer(io.Discard, &contextReader{ctx: ctx, reader: r}, make([]byte, 64<<10))
	if !errors.Is(err, context.Canceled) || n > 64<<10 {
		t.Fatalf("unbounded cancellation n=%d err=%v", n, err)
	}
}

func TestEmptySnapshotSealsAndReopensOffsets(t *testing.T) {
	b, seed := sourceFixture(t)
	root := t.TempDir()
	pin, err := FixtureSourcePin(seed.Repository, seed.Product, root)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = b.IndexFixture(context.Background(), pin, root); err != nil {
		t.Fatal(err)
	}
	cache, _, err := b.openIndex(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if len(cache.offsets) != 0 {
		t.Fatal("empty snapshot contains records")
	}
}
