package environment

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func testIdentity() Identity {
	return Identity{Repository: "Gethe/wow-ui-source", Commit: "0123456789abcdef0123456789abcdef01234567", Client: "retail", GeneratorVersion: GeneratorVersion}
}

func TestBuildLiteralFactsAndDeclarations(t *testing.T) {
	doc := `local API = {
 Name = "DisplayOnly", Type = "System", Namespace = "C_Test",
 Functions = {{ Name = "GetValues", Type = "Function", SecretReturns = true,
   Arguments = {{ Name = "ids", Type = "table", InnerType = "number", Nilable = true }},
   Returns = {{ Name = "result", Type = "ExampleInfo", Nilable = false, ConditionalSecret = true }} }},
 Events = {{ Name = "Changed", Type = "Event", LiteralName = "TEST_CHANGED", SecretPayloads = true,
   Payload = {{ Name = "itemID", Type = "number", Nilable = false, NeverSecret = true }} }},
 Tables = {
  { Name = "ExampleInfo", Type = "Structure", Fields = {{ Name = "names", Type = "table", InnerType = "string", Nilable = true }} },
  { Name = "Choice", Type = "Enumeration", Fields = {{ Name = "First", Type = "Choice", EnumValue = 0 }} },
  { Name = "ChangedCallback", Type = "Callback", Arguments = {{ Name = "value", Type = "number" }} },
 }
}
APIDocumentation:AddDocumentationTable(API)
`
	result, err := Build(context.Background(), testIdentity(), map[string][]byte{"API.lua": []byte(doc)}, Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Facts) != 5 {
		t.Fatalf("facts=%d gaps=%+v", len(result.Facts), result.Coverage.Gaps)
	}
	var function, event APIRecord
	for _, record := range result.Facts {
		if record.Kind == "function" {
			function = record
		}
		if record.Kind == "event" {
			event = record
		}
	}
	if function.Name != "C_Test.GetValues" || function.DisplayGroup != "DisplayOnly" || function.Raw["SecretReturns"] != true || function.Parameters[0].Index != 1 || function.Returns[0].Raw["ConditionalSecret"] != true {
		t.Fatalf("function=%+v", function)
	}
	if event.Name != "TEST_CHANGED" || event.Raw["SecretPayloads"] != true || event.Payload[0].Raw["NeverSecret"] != true {
		t.Fatalf("event=%+v", event)
	}
	defs := string(result.Definitions)
	for _, want := range []string{"---@class ExampleInfo", "---@field names string[]|nil", "---@alias Choice integer", "Enum.Choice = {", "First = 0", "---@alias ChangedCallback fun(value:number)", "function C_Test.GetValues(ids) end"} {
		if !strings.Contains(defs, want) {
			t.Errorf("declarations missing %q:\n%s", want, defs)
		}
	}
	if len(result.Mappings) != 1 || result.Mappings[0].Callable != "C_Test.GetValues" || result.Mappings[0].SourceLine == 0 {
		t.Fatalf("mappings=%+v", result.Mappings)
	}
	if result.Manifest.InputSHA256 == "" || result.Manifest.DefinitionsSHA256 == "" || result.Manifest.FactsSHA256 == "" {
		t.Fatal("missing digests")
	}
}

func TestBuildLegacyCPrefixSystemNamespace(t *testing.T) {
	doc := `local API={Name="C_AuctionHouse",Type="System",Functions={{Name="GetItemSearchResultInfo",Type="Function"}}}; APIDocumentation:AddDocumentationTable(API)`
	result, err := Build(context.Background(), testIdentity(), map[string][]byte{"API.lua": []byte(doc)}, Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Facts) != 1 || result.Facts[0].Name != "C_AuctionHouse.GetItemSearchResultInfo" || result.Facts[0].DisplayGroup != "C_AuctionHouse" || result.Facts[0].Namespace != "C_AuctionHouse" {
		t.Fatalf("legacy namespace: %+v", result.Facts)
	}
	if !strings.Contains(string(result.Definitions), "function C_AuctionHouse.GetItemSearchResultInfo() end") {
		t.Fatalf("missing qualified declaration: %s", result.Definitions)
	}
}

func TestBuildRejectsDynamicOrMutatedMetadata(t *testing.T) {
	cases := map[string]string{
		"field mutation": `local X = { Name="X", Functions={{Name="Safe",Type="Function"}} }; X.Functions = makeFunctions(); APIDocumentation:AddDocumentationTable(X)`,
		"alias escape":   `local X = { Name="X", Functions={{Name="Safe",Type="Function"}} }; local Y = X; APIDocumentation:AddDocumentationTable(X)`,
		"call escape":    `local X = { Name="X", Functions={{Name="Safe",Type="Function"}} }; mutate(X); APIDocumentation:AddDocumentationTable(X)`,
		"mixed table":    `local X = { Name="X", Functions={{Name="Safe",Type="Function", Conditions={"a", X="b"}}} }; APIDocumentation:AddDocumentationTable(X)`,
		"dynamic slot":   `local X = { Name="X", Functions={{Name="Safe",Type="Function", Returns={variable}}} }; APIDocumentation:AddDocumentationTable(X)`,
	}
	for name, doc := range cases {
		t.Run(name, func(t *testing.T) {
			result, err := Build(context.Background(), testIdentity(), map[string][]byte{"X.lua": []byte(doc)}, Limits{})
			if err != nil {
				t.Fatal(err)
			}
			if result.Coverage.Complete || len(result.Coverage.Gaps) == 0 {
				t.Fatalf("false complete: %+v", result)
			}
			if name != "mixed table" && name != "dynamic slot" && len(result.Facts) != 0 {
				t.Fatalf("escaped root emitted facts: %+v", result.Facts)
			}
		})
	}
}

func TestBuildPinnedUnitDocs(t *testing.T) {
	base := os.Getenv("LYCHEEDEV_ENVIRONMENT_TEST_DOCS")
	if base == "" {
		t.Skip("set LYCHEEDEV_ENVIRONMENT_TEST_DOCS to locally downloaded pinned documentation directory")
	}
	for _, name := range []string{"retail-UnitDocumentation.lua", "forever-UnitDocumentation.lua", "retail-ContainerDocumentation.lua", "retail-AuctionHouseEnumsDocumentation.lua"} {
		t.Run(name, func(t *testing.T) {
			data, err := os.ReadFile(filepath.Join(base, name))
			if err != nil {
				t.Fatal(err)
			}
			identity := testIdentity()
			if strings.HasPrefix(name, "forever") {
				identity.Client = "forever"
				identity.Commit = "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069"
			} else {
				identity.Commit = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"
			}
			result, err := Build(context.Background(), identity, map[string][]byte{strings.TrimPrefix(name, "retail-"): data}, Limits{})
			if err != nil {
				t.Fatal(err)
			}
			if len(result.Facts) == 0 || len(result.Mappings) == 0 && !strings.Contains(name, "Enums") {
				t.Fatalf("missing facts or mappings: %+v", result.Coverage)
			}
			if strings.Contains(name, "Unit") {
				var health *APIRecord
				for i := range result.Facts {
					if result.Facts[i].Kind == "function" && result.Facts[i].Name == "UnitHealth" {
						health = &result.Facts[i]
						break
					}
				}
				if health == nil || health.DisplayGroup != "Unit" || health.Namespace != "" || len(health.Returns) == 0 {
					t.Fatalf("UnitHealth unavailable: %+v", result.Coverage)
				}
				if strings.Contains(string(result.Definitions), "function Unit.UnitHealth") {
					t.Fatal("display group incorrectly became namespace")
				}
			}
			if strings.Contains(name, "Container") && !strings.Contains(string(result.Definitions), "function C_Container.ContainerIDToInventoryID") {
				t.Fatal("missing C_Container namespace")
			}
			if strings.Contains(name, "Enums") && !strings.Contains(string(result.Definitions), "Enum.AuctionHouseError = {") {
				t.Fatal("missing runtime enum table")
			}
			t.Logf("%s facts=%d mappings=%d definitions=%d gaps=%d unknown=%d", name, len(result.Facts), len(result.Mappings), len(result.Definitions), len(result.Coverage.Gaps), len(result.Coverage.UnknownTypes))
		})
	}
}
