package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"testing"
)

func TestGeneratedAPISystemNameIsNotCallableNamespace(t *testing.T) {
	data := []byte(`local UnitDocumentation = { Type = "System", Name = "Unit", Namespace = "", Functions = {{ Type = "Function", Name = "UnitHealth", Arguments = {{ Name = "unit", Type = "string" }} }} }; APIDocumentation:AddDocumentationTable(UnitDocumentation)`)
	facts, err := AnalyzeDocument(context.Background(), "Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua", data)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, d := range facts.Declarations {
		if d.Category == "api-function" {
			if d.Name != "UnitHealth" {
				t.Fatalf("display group became callable namespace: %+v", d)
			}
			found = true
		}
	}
	if !found {
		t.Fatalf("global UnitHealth was not extracted: %+v", facts.Declarations)
	}
}

func TestFileFactsCacheSeparatesGeneratedAPIParserMode(t *testing.T) {
	b, _ := sourceFixture(t)
	data := []byte(`return { Type = "System", Name = "Unit", Functions = {{ Type = "Function", Name = "UnitHealth" }} }`)
	sum := sha256.Sum256(data)
	digest := hex.EncodeToString(sum[:])
	ctx := context.Background()
	ordinary, err := b.fileFacts(ctx, "Other/Unit.lua", data, digest)
	if err != nil {
		t.Fatal(err)
	}
	generated, err := b.fileFacts(ctx, "Interface/AddOns/Blizzard_APIDocumentationGenerated/Unit.lua", data, digest)
	if err != nil {
		t.Fatal(err)
	}
	if len(ordinary.Declarations) != 0 || len(generated.Declarations) == 0 {
		t.Fatalf("parser mode cache collision: ordinary=%+v generated=%+v", ordinary.Declarations, generated.Declarations)
	}
}
