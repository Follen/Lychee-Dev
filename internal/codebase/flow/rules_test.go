package flow

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"strings"
	"testing"
)

func TestExtractRulesFromGeneratedLocalTable(t *testing.T) {
	path := "Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua"
	doc := []byte(`local Unit = {
 Name = "Unit", Type = "System", Functions = {
  { Name = "UnitHealth", Type = "Function", SecretReturns = true,
    SecretArguments = "AllowedWhenUntainted",
    Arguments = {{Name = "unit", Type = "UnitToken"}},
    Returns = {{Name = "result", Type = "number"}} },
 }
}
APIDocumentation:AddDocumentationTable(Unit)
`)
	documents := map[string][]byte{path: doc}
	rules, err := ExtractRules(context.Background(), "retail", strings.Repeat("a", 40), DigestDocuments(documents), "1", documents)
	if err != nil {
		t.Fatal(err)
	}
	unit, ok := rules.Calls["UnitHealth"]
	if !ok || len(rules.Calls) != 1 || unit.Returns[1].State != "possible" || unit.Arguments[1].Secret != "conditional" || unit.Raw["SecretArguments"] != "AllowedWhenUntainted" || unit.Path != path || unit.Line < 1 {
		t.Fatalf("local metadata lost identity/conditions: %+v", rules)
	}
	if _, ok := rules.Calls["Unit.UnitHealth"]; ok {
		t.Fatal("documentation group mistaken for Lua namespace")
	}
	bad := append([]byte{}, doc...)
	bad = append(bad, []byte("Unit = {}\n")...)
	mutated := map[string][]byte{path: bad}
	if _, err := ExtractRules(context.Background(), "retail", strings.Repeat("a", 40), DigestDocuments(documents), "1", mutated); err == nil {
		t.Fatal("accepted changed metadata digest")
	}
}

// Supply local copies of the exact pinned Gethe source-mirror files to run
// this offline real-source check. CI does not fetch remote GitHub content.
func TestExtractPinnedUnitDocumentationWhenAvailable(t *testing.T) {
	for _, variant := range []struct{ name, env, commit, sha string }{
		{"retail", "LYCHEEDEV_FLOW_RETAIL_DOC", "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b", "a33d4c585bc3458ab1179070778a2ebef5c324bc5c56bee9511a9fa9eee04a60"},
		{"forever", "LYCHEEDEV_FLOW_FOREVER_DOC", "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069", "3cefaa815d7e207465a9086b60ee92242d5bdef076af13a227591d23f71662ce"},
	} {
		t.Run(variant.name, func(t *testing.T) {
			file := os.Getenv(variant.env)
			if file == "" {
				t.Skip("set " + variant.env + " to the pinned UnitDocumentation.lua")
			}
			raw, err := os.ReadFile(file)
			if err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(raw)
			if hex.EncodeToString(digest[:]) != variant.sha {
				t.Fatalf("pinned source bytes changed: %x", digest)
			}
			path := "Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua"
			documents := map[string][]byte{path: raw}
			rules, err := ExtractRules(context.Background(), variant.name, variant.commit, DigestDocuments(documents), "1", documents)
			if err != nil {
				t.Fatal(err)
			}
			unit, ok := rules.Calls["UnitHealth"]
			if !ok || unit.Returns[1].State != "possible" || unit.Arguments[1].Secret != "conditional" || unit.Raw["SecretReturns"] != true || unit.Raw["SecretArguments"] != "AllowedWhenUntainted" {
				t.Fatalf("pinned UnitHealth rule missing/partial: %+v, issues=%+v", unit, rules.Issues)
			}
			if variant.name == "retail" {
				use := rules.Calls["UnitPower"]
				if use.Arguments[2].Secret != "conditional" {
					t.Fatalf("pinned constrained use missing: %+v", use)
				}
				result, err := Analyze(context.Background(), Request{Files: map[string][]byte{"main.lua": []byte("local amount=UnitHealth('player')\nUnitPower('player',amount)\n")}, Target: Target{Path: "main.lua"}, Rules: rules})
				if err != nil || len(result.Findings) != 1 || result.Findings[0].State != "possible" || result.Findings[0].RulePath != path || result.Findings[0].Argument != 2 {
					t.Fatalf("real pinned rule did not carry a bounded candidate path: %+v, %v", result, err)
				}
			}
		})
	}
}

func TestEventRulesNeverBecomeCallableAndDuplicatesStayAmbiguous(t *testing.T) {
	path := "Generated.lua"
	doc := []byte(`local API={Name="Demo",Type="System",Functions={{Name="Sink",Type="Function",SecretArguments="AllowedWhenUntainted",Arguments={{Name="value",Type="number"}}},{Name="Sink",Type="Function",SecretArguments="AllowedWhenTainted",Arguments={{Name="value",Type="number"}}},{Name="Sink",Type="Function",SecretArguments="AllowedWhenUntainted",Arguments={{Name="value",Type="number"}}}},Events={{Name="EVENT_X",Type="Event",SecretPayloads=true,Payload={{Name="value",Type="number"}}}}}
APIDocumentation:AddDocumentationTable(API)
`)
	docs := map[string][]byte{path: doc}
	rules, err := ExtractRules(context.Background(), "retail", strings.Repeat("a", 40), DigestDocuments(docs), "1", docs)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := rules.Calls["Sink"]; ok {
		t.Fatalf("ambiguous third declaration was promoted: %+v", rules.Calls)
	}
	if _, ok := rules.Calls["EVENT_X"]; ok {
		t.Fatal("event payload became callable")
	}
	if rules.Events["EVENT_X"].Returns[1].State != "possible" {
		t.Fatalf("event payload facts lost: %+v", rules.Events)
	}
	if !containsIssue(rules.Issues, "event_payload_mapping_unavailable") {
		t.Fatalf("missing event routing boundary: %+v", rules.Issues)
	}
}

func containsIssue(issues []Boundary, code string) bool {
	for _, issue := range issues {
		if issue.Code == code {
			return true
		}
	}
	return false
}
