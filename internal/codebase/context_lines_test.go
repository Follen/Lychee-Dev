package codebase

import (
	"context"
	"fmt"
	"runtime"
	"strings"
	"testing"
)

func TestContextSnippetsReuseBoundedLineIndex(t *testing.T) {
	const n = 200
	const padding = 20000
	var body strings.Builder
	body.WriteString(strings.Repeat("-- Padding data in a large generated Lua source document.\n", padding))
	body.WriteString("function CalledTarget() end\n")
	for i := 0; i < n; i++ {
		fmt.Fprintf(&body, "function Caller%d()\n CalledTarget()\nend\n", i)
	}
	b, pin := indexedSearchFixture(t, map[string]string{"Large.lua": body.String()})
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	// Exclude already-built source metadata/index and source-read cost from the
	// allocation measurement; the context renderer still processes this document.
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	if _, _, err = cache.document(ctx, "Large.lua"); err != nil {
		t.Fatal(err)
	}
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	got, err := b.Context(ctx, "PIN-TEST", pin, ContextQuery{Symbol: "CalledTarget", Depth: 1, Limit: 200, MaxBytes: 1 << 20, MaxLines: 2000}, ResearchOptions{})
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	rendered := 0
	for _, s := range got.Snippets {
		rendered += len(s.Text)
	}
	t.Logf("source=%d bytes snippets=%d output=%d bytes allocations=%d bytes documents=%d line index builds=%d partial=%v cursor=%v", body.Len(), len(got.Snippets), rendered, after.TotalAlloc-before.TotalAlloc, cache.work.DocumentReads, cache.work.LineIndexBuilds, got.Truncated, got.NextCursor != "")
	if len(got.Snippets) != n {
		t.Fatalf("snippets=%d want=%d", len(got.Snippets), n)
	}
	if cache.work.LineIndexBuilds != 1 || cache.work.DocumentReads != 1 || cache.lineMemoBytes > maxLineMemoBytes {
		t.Fatalf("repeated document work: %+v, line memo=%d", cache.work, cache.lineMemoBytes)
	}
	// A generous whole-request allocation ceiling, not an exact allocator/timing assertion.
	// The previous per-snippet full-file split allocates more than 280 MiB here.
	if after.TotalAlloc-before.TotalAlloc > 64<<20 {
		t.Fatalf("context allocated %d bytes", after.TotalAlloc-before.TotalAlloc)
	}
	for _, snippet := range got.Snippets {
		if !strings.Contains(snippet.Text, "CalledTarget()") {
			t.Fatalf("wrong snippet: %+v", snippet)
		}
	}
}

func TestContextIndexedExcerptPreservesLineAndCursorSemantics(t *testing.T) {
	for _, tc := range []struct{ name, raw, want string }{
		{"mixed", "one\r\n世界\r\r\nbar\rinside\nend\r", "1: one\n2: 世界\n3: bar\rinside\n4: end"},
		{"trailing", "one\r\n", "1: one\n2: "},
		{"bare-cr", "one\r\r", "1: one\r"},
		{"empty", "", "1: "},
	} {
		t.Run(tc.name, func(t *testing.T) {
			data := []byte(tc.raw)
			c := &snapshotCache{}
			starts, err := c.documentLineStarts(context.Background(), tc.name, data)
			if err != nil {
				t.Fatal(err)
			}
			got, last, _, _, done, err := contextExcerpt(data, starts, 1, 100, 0, 4096, 100)
			if err != nil || !done || got != tc.want || last != len(starts) {
				t.Fatalf("text=%q last=%d done=%v err=%v", got, last, done, err)
			}
			_, _, line, offset, done, err := contextExcerpt(data, starts, 1, 100, 0, 4096, 1)
			if err != nil || line != 2 || offset != 0 || done != (len(starts) == 1) {
				t.Fatalf("line limit: line=%d offset=%d done=%v err=%v", line, offset, done, err)
			}
		})
	}
	data := []byte("世界ab\r\n")
	starts := []uint32{0, uint32(len(data))}
	line, offset, joined := 1, 0, ""
	for i := 0; i < 10; i++ {
		part, _, nextLine, nextByte, done, err := contextExcerpt(data, starts, line, 1, offset, 4, 1)
		if err != nil {
			t.Fatal(err)
		}
		joined += part
		if done {
			break
		}
		line, offset = nextLine, nextByte
	}
	if joined != "1: 世界ab" {
		t.Fatalf("UTF-8 continuation=%q", joined)
	}
	if _, _, _, _, _, err := contextExcerpt(data, starts, 1, 1, 100, 10, 1); err != ErrInvalidSearchCursor {
		t.Fatalf("invalid cursor=%v", err)
	}
}
