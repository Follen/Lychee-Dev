package codebase

import (
	"context"
	"strconv"
	"testing"
)

func TestReferenceSitesResolveManyNamesInOneIndexScan(t *testing.T) {
	wanted := map[referenceLookupKey]bool{}
	for i := 0; i < 100; i++ {
		wanted[referenceLookupKey{"C_Test.Func" + strconv.Itoa(i), "call"}] = true
	}
	wanted[referenceLookupKey{"TEST_EVENT", "event-registration"}] = true
	wanted[referenceLookupKey{"TestFrame", "xml-inherits"}] = true
	entries := []sourceRecord{
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Name: "C_Test.Func0", Category: "api-function", Path: "B.lua", Line: 9}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Name: "C_Test.Func0", Category: "api-function", Path: "A.lua", Line: 3}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Name: "TEST_EVENT", Category: "api-event", Path: "API.lua", Line: 2}},
		{Kind: "symbol", Symbol: &SymbolMatch{Kind: "declaration", Name: "TestFrame", Category: "xml-Frame", Path: "UI.xml", Line: 1}},
	}
	scans := 0
	sites, err := collectReferenceSites(context.Background(), wanted, func(_ context.Context, visit func(sourceRecord) error) error {
		scans++
		for _, entry := range entries {
			if err := visit(entry); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if scans != 1 {
		t.Fatalf("expected one index pass for %d names, got %d", len(wanted), scans)
	}
	funcSites := sites[referenceLookupKey{"C_Test.Func0", "call"}]
	if len(funcSites) != 2 || funcSites[0].Path != "A.lua" || funcSites[1].Path != "B.lua" {
		t.Fatalf("declaration ordering changed: %+v", funcSites)
	}
	if len(sites[referenceLookupKey{"C_Test.Func1", "call"}]) != 0 ||
		len(sites[referenceLookupKey{"TEST_EVENT", "event-registration"}]) != 1 ||
		len(sites[referenceLookupKey{"TestFrame", "xml-inherits"}]) != 1 {
		t.Fatalf("category or absence resolution changed: %+v", sites)
	}
}
