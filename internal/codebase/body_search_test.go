package codebase

import (
	"bufio"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestBodySearchPreservesSupportedFilenames(t *testing.T) {
	const text = "filename_body_needle"
	const source = "-- heading\n-- filename_body_needle: left:right\nreturn 1\n"
	files := map[string]string{
		"测试.lua":            source,
		"données/café.lua":  source,
		"quoted\"name.lua":  source,
		"tab\tname.lua":     source,
		"ordinary name.LUA": source,
		"README.md":         source,
	}
	b, pin := indexedSearchFixture(t, files)
	ctx := context.Background()
	// The moving branch is not the indexed source identity.
	other := testCommit(t, b, pin, map[string]string{"Other.lua": "return 2\n"}, "different tree")
	if _, err := gitBytes(ctx, b.mirror(pin.Repository), 4096, "update-ref", pin.RequestedRef, other.ExactCommit); err != nil {
		t.Fatal(err)
	}
	for name, source := range files {
		if name == "README.md" {
			continue
		}
		span, err := b.ReadSpan(ctx, pin, SpanQuery{Path: name, FirstLine: 1, LineCount: 3})
		if err != nil || span.Text != source {
			t.Fatalf("read %q: %+v err=%v", name, span, err)
		}
	}
	for _, mode := range []SearchMode{SearchModePrecise, SearchModeExploratory} {
		t.Run(string(mode), func(t *testing.T) {
			got, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: mode, Text: text})
			if err != nil || !got.Complete || got.Truncated || len(got.Results) != len(files)-1 || got.ResolvedCommit != pin.ExactCommit {
				t.Fatalf("search: %+v err=%v", got, err)
			}
			seen := map[string]bool{}
			for _, match := range got.Results {
				if _, ok := files[match.Path]; !ok || match.Path == "README.md" || seen[match.Path] || match.Line != 2 || match.MatchedBy != "body_text" || match.ContentHash == "" || !strings.Contains(match.Excerpt, "2: -- "+text+": left:right") {
					t.Fatalf("wrong body result: %+v", match)
				}
				seen[match.Path] = true
			}
		})
	}
}

func TestBodyCandidatesFramingAndLimits(t *testing.T) {
	const text = "framing_body_needle"
	b, pin := indexedSearchFixture(t, map[string]string{
		"Text.lua": "-- heading\n-- framing_body_needle: left:right\r\n-- framing_body_needle: final",
		"Nul.lua":  "-- framing_body_needle: before\x00after\n",
	})
	ctx := context.Background()
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	for _, test := range []struct {
		query string
		limit int
		want  int
		cut   bool
	}{
		{text, 1, 1, true},
		{text, 2, 2, false},
		{"no_body_match", 2, 0, false},
	} {
		rows, cut, err := cache.bodyCandidates(ctx, test.query, "lua", test.limit)
		if err != nil || len(rows) != test.want || cut != test.cut {
			t.Fatalf("query=%q limit=%d: rows=%+v cut=%t err=%v", test.query, test.limit, rows, cut, err)
		}
		for i, row := range rows {
			want := []string{"-- framing_body_needle: left:right", "-- framing_body_needle: final"}[i]
			if row.symbol.Path != "Text.lua" || row.symbol.Line != i+2 || row.symbol.Signature != want {
				t.Fatalf("framing changed: %+v", row)
			}
		}
	}
	// -I normally excludes NUL-containing blobs. Git's explicit text attribute
	// can admit one, so only the first two NULs delimit header fields.
	if err := os.WriteFile(filepath.Join(b.mirror(pin.Repository), "info", "attributes"), []byte("*.lua diff\n"), 0600); err != nil {
		t.Fatal(err)
	}
	rows, cut, err := cache.bodyCandidates(ctx, text, "lua", 3)
	if err != nil || cut || len(rows) != 3 {
		t.Fatalf("text-attributed NUL: rows=%+v cut=%t err=%v", rows, cut, err)
	}
	if rows[0].symbol.Path != "Nul.lua" || rows[0].symbol.Line != 1 || rows[0].symbol.Signature != "-- framing_body_needle: before\x00after" {
		t.Fatalf("snippet NUL lost: %+v", rows[0])
	}
}

func TestBodyCandidatesRetainOutputAndLineBudgets(t *testing.T) {
	for _, test := range []struct {
		name, source string
		wantCut      bool
		wantErr      error
	}{
		{"output", strings.Repeat("-- budget_body_needle "+strings.Repeat("x", 1100)+"\n", 4000), true, nil},
		{"line", "-- budget_body_needle " + strings.Repeat("x", 1<<20) + "\n", false, bufio.ErrTooLong},
	} {
		t.Run(test.name, func(t *testing.T) {
			b, pin := indexedSearchFixture(t, map[string]string{"测试.lua": test.source})
			ctx := context.Background()
			cache, _, err := b.openIndex(ctx, pin)
			if err != nil {
				t.Fatal(err)
			}
			defer cache.Close()
			rows, cut, err := cache.bodyCandidates(ctx, "budget_body_needle", "lua", 10000)
			if !errors.Is(err, test.wantErr) || cut != test.wantCut || test.wantErr == nil && (len(rows) == 0 || len(rows) >= 4000) {
				t.Fatalf("rows=%d cut=%t err=%v", len(rows), cut, err)
			}
		})
	}
}
