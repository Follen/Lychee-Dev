package codebase

import (
	"context"
	"strconv"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

func TestCompatibilityResolvesManyNamesInOneIndexScan(t *testing.T) {
	usage := []ReferenceUsage{
		{Kind: "api", Name: "C_Test.First", File: "A.lua", Line: 2},
		{Kind: "api", Name: "C_Test.Second", File: "A.lua", Line: 3},
		{Kind: "api", Name: "C_Test.First", File: "B.lua", Line: 8},
		{Kind: "api-candidate", Name: "AddonOwned", File: "B.lua", Line: 9},
		{Kind: "event", Name: "TEST_EVENT", File: "A.lua", Line: 4},
		{Kind: "template", Name: "TestTemplate", File: "A.xml", Line: 1},
		{Kind: "frame-type", Name: "Frame", File: "A.xml", Line: 2},
	}
	records := []sourceRecord{
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Category: "api-function", Name: "C_Test.First", Path: "API.lua", Line: 10}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Category: "api-function", Name: "C_Test.Second", Path: "API.lua", Line: 20}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Category: "api-event", Name: "TEST_EVENT", Path: "API.lua", Line: 30}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Category: "xml-Frame", Name: "TestTemplate", Path: "UI.xml", Line: 3}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "header", Category: "toc-header", Name: "Interface", Target: "120100", Path: "UI.toc", Line: 1}},
	}
	for i := 0; i < 64; i++ {
		usage = append(usage, ReferenceUsage{Kind: "api", Name: "Missing" + strconv.Itoa(i), File: "Many.lua", Line: i + 1})
	}
	callCount := 0
	scan := func(ctx context.Context, visit func(sourceRecord) error) error {
		callCount++
		for _, record := range records {
			if err := visit(record); err != nil {
				return err
			}
		}
		return nil
	}
	pin := selection.SourcePin{Repository: "wow-ui-source", Product: "retail"}
	facts, unresolved, diagnostics, err := lookupCompatibilityWithScan(context.Background(), pin, "PIN-TEST", usage, "120100", scan)
	if err != nil {
		t.Fatal(err)
	}
	if callCount != 1 {
		t.Fatalf("expected one index scan for %d distinct references, got %d", len(usage), callCount)
	}
	if len(facts) != len(usage)-1+1 || len(unresolved) != 1 || len(diagnostics) != 64 {
		t.Fatalf("unexpected resolution counts: facts=%d unresolved=%d diagnostics=%d", len(facts), len(unresolved), len(diagnostics))
	}
	if facts[0].Evidence["usage"].(map[string]any)["file"] != "A.lua" || facts[2].Evidence["usage"].(map[string]any)["file"] != "B.lua" {
		t.Fatal("repeated API reference lost caller-specific evidence")
	}
	for _, kind := range []string{"api", "event", "template", "frame-type", "interface"} {
		found := false
		for _, fact := range facts {
			if fact.Kind == kind && fact.Exists {
				found = true
				break
			}
		}
		if !found {
			t.Fatalf("missing resolved %s fact", kind)
		}
	}
}
