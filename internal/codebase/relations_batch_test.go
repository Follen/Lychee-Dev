package codebase

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/luals"
	"strings"
	"testing"
)

func TestRelationBatchPreserves128OutgoingSites(t *testing.T) {
	var text strings.Builder
	text.WriteString("function Caller()\n")
	edges := []SymbolMatch{}
	for i := 0; i < 128; i++ {
		target := fmt.Sprintf("Target%03d", i)
		fmt.Fprintf(&text, " %s()\n", target)
		edges = append(edges, SymbolMatch{Name: "Caller", Target: target, Category: "call", Path: "probe.lua", Line: i + 2})
	}
	text.WriteString("end\n")
	b, pin := indexedSearchFixture(t, map[string]string{"probe.lua": text.String()})
	ctx, cleanup := sourceQueryContext(context.Background())
	defer cleanup()
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	outgoing, sites, cut := semanticOutgoingQueries(ctx, cache, SymbolMatch{Name: "Caller", Path: "probe.lua", Line: 1, EndLine: 130}, edges, 128)
	if cut || len(outgoing) != 128 || len(sites) != 128 {
		t.Fatalf("outgoing=%d sites=%d cut=%v", len(outgoing), len(sites), cut)
	}
	queries := append([]luals.Query{{Kind: luals.References, Path: "probe.lua"}, {Kind: luals.Hover, Path: "probe.lua"}}, outgoing...)
	batches := luals.SplitQueries(queries)
	if len(batches) != 2 || len(batches[0]) != 128 || len(batches[1]) != 2 {
		t.Fatalf("lost sites in composed request: %v", batches)
	}
}
func TestProjectedSemanticAnalysisIsIndependent(t *testing.T) {
	all := luals.Analysis{State: "partial", Results: []luals.QueryResult{{State: "incomplete"}, {State: "complete"}}}
	outgoing := projectedSemanticAnalysis(all, all.Results[1:])
	if outgoing.State != "complete" {
		t.Fatal("references truncation incorrectly marked outgoing partial")
	}
	incoming := projectedSemanticAnalysis(all, all.Results[:1])
	if incoming.State != "partial" {
		t.Fatal("truncated references marked complete")
	}
}
