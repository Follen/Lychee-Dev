package codebase

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestSourceBatchSharesSnapshotAndPreservesIndividualPages(t *testing.T) {
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	request := BatchRequest{Requests: []BatchItem{
		{ID: "find", Query: &SearchQuery{Text: "Widget", Limit: 1}},
		{ID: "uses", Refs: &RelationQuery{Symbol: "WidgetFactory", Limit: 1}},
		{ID: "read", Context: &ContextQuery{Symbol: "WidgetFactory", Depth: 3, Limit: 1, MaxBytes: 32}},
	}}
	result, err := b.batch(ctx, "PIN", pin, request, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	wantQuery, err := b.Search(context.Background(), "PIN", pin, *request.Requests[0].Query)
	if err != nil {
		t.Fatal(err)
	}
	wantRefs, err := b.Relate(context.Background(), "PIN", pin, *request.Requests[1].Refs, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	wantContext, err := b.Context(context.Background(), "PIN", pin, *request.Requests[2].Context, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(result.Results, []BatchItemResult{{ID: "find", Query: &wantQuery}, {ID: "uses", Refs: &wantRefs}, {ID: "read", Context: &wantContext}}) {
		t.Fatal("batch changed answers, coverage or cursors")
	}
	if !result.Truncated || result.Complete || result.Results[0].Query.NextCursor == "" {
		t.Fatalf("lost per-item continuation: %+v", result)
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if cache.work.VerificationPasses != 1 || cache.work.LookupBuilds != 1 || cache.work.GitBatchStarts != 1 {
		t.Fatalf("repeated preparation: %+v", cache.work)
	}
	t.Logf("query + refs + context: %+v", cache.work)
	closeQuery()
	if cache.records != nil || cache.batch != nil {
		t.Fatal("batch retained resources after close")
	}
}

func TestSourceBatchRejectsMalformedRequests(t *testing.T) {
	file := filepath.Join(t.TempDir(), "requests.json")
	for _, raw := range []string{
		`{"requests":[]}`, `{"requests":[{}]}`,
		`{"requests":[{"query":{"text":"Foo"},"refs":{"symbol":"Foo"}}]}`,
		`{"requests":[{"query":{"text":"Foo","unexpected":1}}]}`,
		`{"requests":[{"query":{"text":"Foo"}}]} {}`,
		strings.Repeat(" ", (64<<10)+1),
		`{"requests":[` + strings.TrimSuffix(strings.Repeat(`{"query":{"text":"Foo"}},`, 33), ",") + `]}`,
	} {
		if err := os.WriteFile(file, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := ReadSourceBatch(file); !errors.Is(err, ErrInvalidBatch) {
			t.Fatalf("invalid batch accepted: %.100s %v", raw, err)
		}
	}
}

func TestLookupSearchAndDeepRelationsAvoidUnrelatedMetadata(t *testing.T) {
	var source strings.Builder
	source.WriteString("function Root() Namespace.Selected() end\nfunction Namespace.Selected() Target() end\nfunction Target() Root() end\n")
	for i := 0; i < 2000; i++ {
		fmt.Fprintf(&source, "function Unrelated%d() end\n", i)
	}
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": source.String()})
	ctx, closeQuery := sourceQueryContext(context.Background())
	defer closeQuery()
	for _, term := range []string{"Selected", "Select", "Namespace.Selected"} {
		result, err := b.Search(ctx, "PIN", pin, SearchQuery{Text: term})
		if err != nil || len(result.Results) != 1 || result.Results[0].Name != "Namespace.Selected" {
			t.Fatalf("qualified name/prefix lookup %s: %+v %v", term, result, err)
		}
	}
	result, err := b.Context(ctx, "PIN", pin, ContextQuery{Symbol: "Root", Depth: 4, Limit: 100}, ResearchOptions{})
	if err != nil || len(result.Snippets) == 0 {
		t.Fatalf("context: %+v %v", result, err)
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if cache.work.LookupBuilds != 1 || cache.work.LookupEntries < 2000 || cache.work.MetadataVisits > 100 {
		t.Fatalf("query traversed unrelated metadata: %+v", cache.work)
	}
	t.Logf("three symbol lookups + depth 4: %+v", cache.work)
}

func TestDocumentLinePositionsKeepUTF16AndAmbiguity(t *testing.T) {
	b, pin := indexedSearchFixture(t, map[string]string{"a.lua": "-- 😀 Target()\r\n-- Target() Target()\n"})
	cache, _, err := b.openIndex(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	position, err := symbolPosition(context.Background(), cache, SymbolMatch{Path: "a.lua", Name: "Target", Line: 1})
	if err != nil || position.Line != 0 || position.Character != 6 {
		t.Fatalf("UTF-16 position: %+v %v", position, err)
	}
	if _, err := symbolPosition(context.Background(), cache, SymbolMatch{Path: "a.lua", Name: "Target", Line: 2}); err == nil || !strings.Contains(err.Error(), "ambiguous") {
		t.Fatal("ambiguous position accepted", err)
	}
	if line, err := cache.documentLine(context.Background(), "a.lua", 3); err != nil || line != "" {
		t.Fatalf("trailing empty line: %q %v", line, err)
	}
	if _, err := cache.documentLine(context.Background(), "a.lua", 4); err == nil || !strings.Contains(err.Error(), "out_of_range") {
		t.Fatal("out of range accepted", err)
	}
	if cache.work.LineIndexBuilds != 1 {
		t.Fatal("line offsets rebuilt", cache.work)
	}
}
