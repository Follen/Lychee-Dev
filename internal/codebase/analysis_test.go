package codebase

import (
	"context"
	"reflect"
	"strings"
	"testing"
)

func analyzeFixture(t *testing.T, name, source string) DocumentFacts {
	t.Helper()
	facts, err := AnalyzeDocument(context.Background(), name, []byte(source))
	if err != nil {
		t.Fatalf("AnalyzeDocument(%q): %v", name, err)
	}
	return facts
}

func hasDeclaration(facts DocumentFacts, want Declaration) bool {
	for _, declaration := range facts.Declarations {
		if declaration == want {
			return true
		}
	}
	return false
}

func hasRelationship(facts DocumentFacts, want Relationship) bool {
	for _, relationship := range facts.Relationships {
		if relationship == want {
			return true
		}
	}
	return false
}

func TestAnalyzeDocumentLuaFacts(t *testing.T) {
	data := `function Addon.Start(value)
  return value
end
local function helper()
end
Addon.Start(1)
local handlers = {}
handlers[method]()
frame:RegisterEvent(eventName)
frame:RegisterEvent("PLAYER_LOGIN")
frame:RegisterUnitEvent("UNIT_AURA", "player")
`
	facts := analyzeFixture(t, "Interface/AddOns/Test/Core.lua", data)

	if got, want := facts.Declarations, []Declaration{
		{Name: "Addon.Start", Category: "function", Line: 1, EndLine: 3, Signature: "function Addon.Start(value)"},
		{Name: "helper", Category: "local-function", Line: 4, EndLine: 5, Signature: "local function helper()"},
	}; !reflect.DeepEqual(got, want) {
		t.Fatalf("declarations = %#v, want %#v", got, want)
	}

	for _, want := range []Relationship{
		{From: "<file>", To: "Addon.Start", Category: "call", Confidence: "inferred", Line: 6},
		{From: "<file>", To: "<dynamic>", Category: "call", Confidence: "dynamic-unresolved", Line: 8},
		{From: "<file>", To: "frame:RegisterEvent", Category: "call", Confidence: "inferred", Line: 9},
		{From: "<file>", To: "<dynamic>", Category: "event-registration", Confidence: "dynamic-unresolved", Line: 9},
		{From: "<file>", To: "frame:RegisterEvent", Category: "call", Confidence: "inferred", Line: 10},
		{From: "<file>", To: "PLAYER_LOGIN", Category: "event-registration", Confidence: "exact", Line: 10},
		{From: "<file>", To: "frame:RegisterUnitEvent", Category: "call", Confidence: "inferred", Line: 11},
		{From: "<file>", To: "UNIT_AURA", Category: "event-registration", Confidence: "exact", Line: 11},
	} {
		if !hasRelationship(facts, want) {
			t.Errorf("missing relationship %#v in %#v", want, facts.Relationships)
		}
	}
	if len(facts.Diagnostics) != 0 {
		t.Fatalf("unexpected diagnostics: %#v", facts.Diagnostics)
	}
}

func TestAnalyzeDocumentSyntaxErrorDoesNotExecuteLua(t *testing.T) {
	data := `error("this source must never execute")
function unfinished(
`
	facts := analyzeFixture(t, "Core/Broken.lua", data)
	if len(facts.Diagnostics) == 0 {
		t.Fatal("syntax error was not reported")
	}
	if len(facts.Declarations) != 0 || len(facts.Relationships) != 0 {
		t.Fatalf("syntax-error input produced facts: %#v", facts)
	}
}

func TestAnalyzeDocumentNeverEvaluatesValidCalls(t *testing.T) {
	facts := analyzeFixture(t, "Core/Untrusted.lua", `error("must not execute")`)
	if len(facts.Diagnostics) != 0 || !hasRelationship(facts, Relationship{From: "<file>", To: "error", Category: "call", Confidence: "inferred", Line: 1}) {
		t.Fatalf("valid call was not treated as syntax: %+v", facts)
	}
}

func TestAnalyzeDocumentGeneratedAPIASTMetadata(t *testing.T) {
	data := `return {
  Type = "System",
  Name = "C_Example",
  Namespace = "C_Example",
  Documentation = "system { braces stay in this string }",
  Functions = {
    {
      Name = "DoThing",
      Type = "Function",
      Documentation = "function { description }",
      Arguments = {
        { Name = "input", Type = "string", Nilable = true },
        { Name = "required", Type = "number", Nilable = false },
      },
      Returns = {
        { Name = "result", Type = "bool", Nilable = true },
      },
    },
  },
  Events = {
    {
      Name = "OnSomething",
      Type = "Event",
      LiteralName = "EXAMPLE_EVENT",
      Documentation = "event { description }",
      Payload = {
        { Name = "value", Type = "string", Nilable = true },
      },
    },
  },
}`
	facts := analyzeFixture(t, "Interface/AddOns/Blizzard_APIDocumentationGenerated/ExampleDocumentation.lua", data)

	for _, want := range []Declaration{
		{Name: "C_Example", Category: "api-system", Line: 1, EndLine: 31, Signature: "C_Example"},
		{Name: "C_Example.DoThing", Category: "api-function", Line: 7, EndLine: 18, Signature: "C_Example.DoThing(input?: string, required: number) -> result?: bool"},
		{Name: "C_Example.OnSomething", Category: "api-event", Line: 21, EndLine: 29, Signature: "C_Example.OnSomething(value?: string)"},
		{Name: "EXAMPLE_EVENT", Category: "api-event", Line: 21, EndLine: 29, Signature: "EXAMPLE_EVENT(value?: string)"},
	} {
		if !hasDeclaration(facts, want) {
			t.Errorf("missing declaration %#v in %#v", want, facts.Declarations)
		}
	}
	for _, declaration := range facts.Declarations {
		if declaration.Name == "<dynamic>" || strings.Contains(declaration.Name, "Documentation") {
			t.Errorf("comment/string polluted API declarations: %#v", facts.Declarations)
		}
	}
	for _, want := range []Relationship{
		{From: "C_Example", To: "C_Example.DoThing", Category: "contains", Confidence: "exact", Line: 7},
		{From: "C_Example", To: "C_Example.OnSomething", Category: "contains", Confidence: "exact", Line: 21},
		{From: "EXAMPLE_EVENT", To: "C_Example.OnSomething", Category: "event-name", Confidence: "exact", Line: 21},
	} {
		if !hasRelationship(facts, want) {
			t.Errorf("missing relationship %#v in %#v", want, facts.Relationships)
		}
	}
	if len(facts.Diagnostics) != 0 {
		t.Fatalf("unexpected diagnostics: %#v", facts.Diagnostics)
	}
}

func TestAnalyzeDocumentXMLAttributesAndEmbeddedScript(t *testing.T) {
	first := `<Ui>
  <Frame name="Root" inherits="BaseA, BaseB">
    <Script file="Core/First.lua" />
    <Include file="Core/Second.xml" />
    <OnLoad>
function self:Init()
end
self:Init()
    </OnLoad>
  </Frame>
</Ui>
`
	second := `<Ui>
  <Frame inherits="BaseA, BaseB" name="Root">
    <Script file="Core/First.lua" />
    <Include file="Core/Second.xml" />
    <OnLoad>
function self:Init()
end
self:Init()
    </OnLoad>
  </Frame>
</Ui>
`
	firstFacts := analyzeFixture(t, "Interface/AddOns/Test/First.xml", first)
	secondFacts := analyzeFixture(t, "Interface/AddOns/Test/Second.xml", second)
	if !reflect.DeepEqual(firstFacts.Declarations, secondFacts.Declarations) {
		t.Fatalf("attribute order changed declarations: %#v vs %#v", firstFacts.Declarations, secondFacts.Declarations)
	}
	if !reflect.DeepEqual(firstFacts.Relationships, secondFacts.Relationships) {
		t.Fatalf("attribute order changed relationships: %#v vs %#v", firstFacts.Relationships, secondFacts.Relationships)
	}
	if !reflect.DeepEqual(firstFacts.Loads, secondFacts.Loads) {
		t.Fatalf("attribute order changed loads: %#v vs %#v", firstFacts.Loads, secondFacts.Loads)
	}

	wantLoads := []LoadReference{
		{Path: "Core/First.lua", Line: 3, Kind: "script"},
		{Path: "Core/Second.xml", Line: 4, Kind: "include"},
	}
	if !reflect.DeepEqual(firstFacts.Loads, wantLoads) {
		t.Fatalf("loads = %#v, want %#v", firstFacts.Loads, wantLoads)
	}
	for _, want := range []Relationship{
		{From: "Root", To: "BaseA", Category: "xml-inherits", Confidence: "exact", Line: 2},
		{From: "Root", To: "BaseB", Category: "xml-inherits", Confidence: "exact", Line: 2},
		{From: "Root:OnLoad", To: "self:Init", Category: "call", Confidence: "inferred", Line: 8},
	} {
		if !hasRelationship(firstFacts, want) {
			t.Errorf("missing XML relationship %#v in %#v", want, firstFacts.Relationships)
		}
	}
	if !hasDeclaration(firstFacts, Declaration{Name: "self:Init", Category: "function", Line: 6, EndLine: 7, Signature: "function self:Init()"}) {
		t.Fatalf("embedded function line missing: %#v", firstFacts.Declarations)
	}
	if len(firstFacts.Diagnostics) != 0 {
		t.Fatalf("unexpected diagnostics: %#v", firstFacts.Diagnostics)
	}
}
