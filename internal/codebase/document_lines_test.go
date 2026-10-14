package codebase

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"reflect"
	"runtime"
	"strings"
	"testing"
)

func TestNumberedLinesPreserveTextAndRangeSemantics(t *testing.T) {
	files := map[string]string{
		"Mixed.lua":    "-- first\r\n-- 世界\r\r\n-- bare\rtext\n-- final\r",
		"Trailing.lua": "-- last\r\n",
		"BareCR.lua":   "-- end\r\r",
		"Empty.lua":    "",
	}
	b, pin := indexedSearchFixture(t, files)
	ctx := context.Background()
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	for _, test := range []struct {
		name                 string
		start, end, maxLines int
		want                 string
	}{
		{"Mixed.lua", 1, 10, 0, "1: -- first\n2: -- 世界\n3: -- bare\rtext\n4: -- final"},
		{"Mixed.lua", 2, 1, 0, "2: -- 世界"},
		{"Mixed.lua", 2, 10, 2, "2: -- 世界\n3: -- bare\rtext"},
		{"Mixed.lua", 5, 10, 0, ""},
		{"Trailing.lua", 1, 10, 0, "1: -- last\n2: "},
		{"Trailing.lua", 2, 2, 0, "2: "},
		{"BareCR.lua", 1, 1, 0, "1: -- end\r"},
		{"Empty.lua", 1, 6, 0, "1: "},
	} {
		snippet, hash, err := cache.numberedLines(ctx, test.name, test.start, test.end, test.maxLines)
		wantHash := fmt.Sprintf("%x", sha256.Sum256([]byte(files[test.name])))
		if err != nil || snippet != test.want || hash != wantHash {
			t.Fatalf("%+v: snippet=%q hash=%q err=%v", test, snippet, hash, err)
		}
	}
	// LSP positions retain original line bytes, including CR characters, even
	// when a snippet has already built the shared index.
	if line, err := cache.documentLine(ctx, "Mixed.lua", 2); err != nil || line != "-- 世界\r\r" {
		t.Fatalf("original line changed: %q err=%v", line, err)
	}
	if cache.work.LineIndexBuilds != len(files) || cache.work.DocumentReads != len(files) {
		t.Fatalf("line index not shared: %+v", cache.work)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, _, err := cache.numberedLines(cancelled, "Mixed.lua", 1, 6, 0); !errors.Is(err, context.Canceled) {
		t.Fatalf("cached excerpt ignored cancellation: %v", err)
	}
}

func TestSearchAndInspectKeepSnippetLimits(t *testing.T) {
	const name = "Long.lua"
	source := "function TallSnippet()\r\n" + strings.Repeat("  -- 世界\r\r\n", 90) + "end\r\n"
	b, pin := indexedSearchFixture(t, map[string]string{name: source})
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	got, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "TallSnippet"})
	if err != nil || len(got.Results) != 1 || !got.Complete || got.Truncated {
		t.Fatalf("search: %+v err=%v", got, err)
	}
	lines := []string{"1: function TallSnippet()"}
	for n := 2; n <= 80; n++ {
		lines = append(lines, fmt.Sprintf("%d:   -- 世界", n))
	}
	hash := fmt.Sprintf("%x", sha256.Sum256([]byte(source)))
	if got.Results[0].Excerpt != strings.Join(lines, "\n") || got.Results[0].ContentHash != hash {
		t.Fatalf("80-line search excerpt: %+v", got.Results[0])
	}
	inspected, err := b.InspectTarget(ctx, "PIN-TEST", pin, TargetQuery{Path: name})
	if err != nil || len(inspected.Results) != 1 || !inspected.Complete || inspected.Truncated || inspected.Results[0].Excerpt != strings.Join(lines[:6], "\n") || inspected.Results[0].ContentHash != hash {
		t.Fatalf("six-line inspection: %+v err=%v", inspected, err)
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if cache.work.LineIndexBuilds != 1 || cache.work.DocumentReads != 1 {
		t.Fatalf("search/inspection did not share line index: %+v", cache.work)
	}
}

func TestSearchSnippetsReuseBoundedLineIndex(t *testing.T) {
	const count = 200
	const padding = 20000
	source := strings.Repeat("-- Padding data in a large generated Lua source document.\n", padding) + strings.Repeat("function RepeatedSnippet()\nend\n", count)
	b, pin := indexedSearchFixture(t, map[string]string{"Large.lua": source})
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	got, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "RepeatedSnippet", Limit: count})
	runtime.ReadMemStats(&after)
	if err != nil || len(got.Results) != count || !got.Complete || got.Truncated {
		t.Fatalf("search: results=%d complete=%t truncated=%t err=%v", len(got.Results), got.Complete, got.Truncated, err)
	}
	digest := fmt.Sprintf("%x", sha256.Sum256([]byte(source)))
	for i, match := range got.Results {
		line := padding + 1 + 2*i
		want := fmt.Sprintf("%d: function RepeatedSnippet()\n%d: end", line, line+1)
		if match.Excerpt != want || match.Line != line || match.ContentHash != digest || match.MatchedBy != "exact_symbol" {
			t.Fatalf("snippet %d: %+v want=%q", i, match, want)
		}
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if cache.work.LineIndexBuilds != 1 || cache.work.DocumentReads != 1 || cache.lineMemoBytes > maxLineMemoBytes {
		t.Errorf("repeated document work: %+v memo bytes=%d", cache.work, cache.lineMemoBytes)
	}
	allocated := after.TotalAlloc - before.TotalAlloc
	// Keep a generous whole-request ceiling, rather than timing or exact
	// allocator counts. Splitting this 1.1 MiB document 200 times costs >280 MiB.
	if allocated > 64<<20 {
		t.Errorf("search allocated %d bytes for %d snippets from %d source bytes", allocated, count, len(source))
	}
	t.Logf("source bytes=%d results=%d allocated bytes=%d line index builds=%d", len(source), len(got.Results), allocated, cache.work.LineIndexBuilds)
	first, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "RepeatedSnippet", Limit: 3})
	if err != nil || !first.Truncated || first.Complete || first.NextCursor == "" || !reflect.DeepEqual(first.Results, got.Results[:3]) {
		t.Fatalf("first page: %+v err=%v", first, err)
	}
	next, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "RepeatedSnippet", Limit: 3, Cursor: first.NextCursor})
	if err != nil || !next.Truncated || next.Complete || !reflect.DeepEqual(next.Results, got.Results[3:6]) || cache.work.LineIndexBuilds != 1 {
		t.Fatalf("next page: %+v work=%+v err=%v", next, cache.work, err)
	}
}
