package flow

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

func TestFlowOutputBudgetIncludesMinimumResult(t *testing.T) {
	rules := testRules()
	result, err := Analyze(context.Background(), Request{Files: map[string][]byte{"main.lua": []byte("Use(Source())\n")}, Target: Target{Path: "main.lua"}, Rules: rules, Budget: Budget{MaxOutputBytes: 2048}})
	if err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(result)
	if err != nil || len(raw) > 2048 {
		t.Fatalf("result exceeded output budget: %d, %v", len(raw), err)
	}
	rules.Identity.Client = strings.Repeat("\x01", 256)
	rules.Identity.APICommit = strings.Repeat("\x01", 256)
	rules.Identity.MetadataDigest = strings.Repeat("\x01", 256)
	rules.Identity.Version = strings.Repeat("\x01", 256)
	_, err = Analyze(context.Background(), Request{Files: map[string][]byte{"main.lua": []byte("Use(Source())\n")}, Target: Target{Path: "main.lua"}, Rules: rules, Budget: Budget{MaxOutputBytes: 2048}})
	if !errors.Is(err, ErrBudget) {
		t.Fatalf("oversized minimum result was silently returned: %v", err)
	}
}

func TestFlowBuiltInOperationsExposeUnknownConstraint(t *testing.T) {
	for _, code := range []string{
		"local x=Source()\nlocal y=x+1\n",
		"local x=Source()\nif x>0 then print('yes') end\n",
		"local x=Source()\nlocal y='health:'..x\n",
		"local x=Source()\nlocal y=({})[x]\n",
	} {
		result := investigate(t, map[string]string{"main.lua": code}, Target{Path: "main.lua"})
		if !hasBoundary(result, "unknown_operation_constraint") || result.Coverage.Complete {
			t.Fatalf("unknown built-in operation was treated as complete: %s %+v", code, result)
		}
	}
}

func TestFlowSameLineFunctionsRemainAmbiguous(t *testing.T) {
	result := investigate(t, map[string]string{"main.lua": "function A() Use(Source()) end function B() return nil end\n"}, Target{Path: "main.lua", Line: 1})
	if !hasBoundary(result, "ambiguous_location") || len(result.Findings) != 0 {
		t.Fatalf("same-line target arbitrarily selected: %+v", result)
	}
}

func TestFlowConditionSideEffectInvalidatesBothBranches(t *testing.T) {
	code := "function Demo()\n local t={}\n t.value=Source()\n if issecretvalue(t.value) then return end\n if Mutate() then end\n Use(t.value)\nend\n"
	result := investigate(t, map[string]string{"main.lua": code}, Target{Path: "main.lua", Symbol: "Demo"})
	if !hasBoundary(result, "unknown_side_effect") || !hasBoundary(result, "unknown_argument_constraint") {
		t.Fatalf("condition call left stale guarded branch value: %+v", result)
	}
}

func TestFlowLocalTableShadowRestoresOuterField(t *testing.T) {
	code := "local t={}\nt.value=Source()\ndo local t={} t.value=0 end\nUse(t.value)\n"
	result := investigate(t, map[string]string{"main.lua": code}, Target{Path: "main.lua"})
	if len(result.Findings) != 1 {
		t.Fatalf("block-local table field overwrote outer field: %+v", result)
	}
	stale := investigate(t, map[string]string{"main.lua": "local t={}\nt.value=0\nt={}\nUse(t.value)\n"}, Target{Path: "main.lua"})
	if !hasBoundary(stale, "unknown_argument_constraint") {
		t.Fatalf("table reassignment retained stale field: %+v", stale)
	}
}

func testRules() RuleSet {
	return RuleSet{Identity: RuleIdentity{Client: "retail", APICommit: strings.Repeat("a", 40), MetadataDigest: "fixture", Version: "1"}, Calls: map[string]CallRule{
		"Source":    {Name: "Source", Path: "api.lua", Line: 10, Returns: map[int]SlotRule{1: {State: "secret"}}},
		"Pair":      {Name: "Pair", Path: "api.lua", Line: 20, Returns: map[int]SlotRule{1: {State: "never"}, 2: {State: "secret"}}},
		"Use":       {Name: "Use", Path: "api.lua", Line: 30, Arguments: map[int]ArgumentRule{1: {Secret: "forbidden"}}},
		"Allow":     {Name: "Allow", Path: "api.lua", Line: 40, Arguments: map[int]ArgumentRule{1: {Secret: "allowed"}}},
		"UseSecond": {Name: "UseSecond", Path: "api.lua", Line: 50, Arguments: map[int]ArgumentRule{2: {Secret: "forbidden"}}},
	}}
}

func investigate(t *testing.T, files map[string]string, target Target) Result {
	t.Helper()
	contents := map[string][]byte{}
	for name, body := range files {
		contents[name] = []byte(body)
	}
	result, err := Analyze(context.Background(), Request{Files: contents, Target: target, Rules: testRules()})
	if err != nil {
		t.Fatal(err)
	}
	return result
}

func TestFlowDirectAndReturnSlotEvidence(t *testing.T) {
	direct := investigate(t, map[string]string{"main.lua": "local x = Source()\nUse(x)\n"}, Target{Path: "main.lua"})
	if len(direct.Findings) != 1 || direct.Findings[0].State != "confirmed_candidate" || direct.Findings[0].Steps[0].Path != "api.lua" || direct.Findings[0].Steps[len(direct.Findings[0].Steps)-1].Line != 2 {
		t.Fatalf("direct path lacks rule/original use evidence: %+v", direct)
	}
	slots := investigate(t, map[string]string{"main.lua": "local safe, secret = Pair()\nUse(safe)\nUse(secret)\n"}, Target{Path: "main.lua"})
	if len(slots.Findings) != 1 || slots.Findings[0].Steps[len(slots.Findings[0].Steps)-1].Line != 3 {
		t.Fatalf("return slot confused: %+v", slots)
	}
}

func TestFlowOrdinaryCrossFileArgumentAndReturn(t *testing.T) {
	result := investigate(t, map[string]string{
		"wrapper.lua": "function Wrap(value)\n  return value\nend\n",
		"main.lua":    "local x = Wrap(Source())\nUse(x)\n",
	}, Target{Path: "main.lua"})
	if len(result.Findings) != 1 {
		t.Fatalf("cross-file wrapper not followed: %+v", result)
	}
	seenArgument, seenReturn := false, false
	for _, step := range result.Findings[0].Steps {
		if step.Kind == "argument" {
			seenArgument = true
		}
		if step.Kind == "return" && step.Path == "wrapper.lua" {
			seenReturn = true
		}
	}
	if !seenArgument || !seenReturn {
		t.Fatalf("wrapper provenance missing: %+v", result.Findings[0].Steps)
	}
}

func TestFlowLuaLastArgumentExpansionAndMethodReceiver(t *testing.T) {
	multiple := investigate(t, map[string]string{"main.lua": "UseSecond(Pair())\n"}, Target{Path: "main.lua"})
	if len(multiple.Findings) != 1 || multiple.Findings[0].Argument != 2 {
		t.Fatalf("last-call multi-return argument lost: %+v", multiple)
	}
	method := investigate(t, map[string]string{"main.lua": "function Box:Wrap(v)\n return v\nend\nlocal x=Box:Wrap(Source())\nUse(x)\n"}, Target{Path: "main.lua"})
	if len(method.Findings) != 1 {
		t.Fatalf("implicit method receiver displaced argument: %+v", method)
	}
}

func TestFlowGuardIdentityAndReassignment(t *testing.T) {
	cases := []struct {
		name, code string
		want       int
	}{
		{"same value", "function Demo()\n local x=Source()\n if issecretvalue(x) then return end\n Use(x)\nend\n", 0},
		{"wrong value", "function Demo()\n local x=Source()\n local y=Source()\n if issecretvalue(y) then return end\n Use(x)\nend\n", 1},
		{"reassigned", "function Demo()\n local x=Source()\n if issecretvalue(x) then return end\n x=Source()\n Use(x)\nend\n", 1},
		{"shadowed builtin", "local issecretvalue=function() return false end\nfunction Demo()\n local x=Source()\n if issecretvalue(x) then return end\n Use(x)\nend\n", 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			result := investigate(t, map[string]string{"main.lua": tc.code}, Target{Path: "main.lua", Symbol: "Demo"})
			if len(result.Findings) != tc.want {
				t.Fatalf("findings=%d want %d: %+v", len(result.Findings), tc.want, result)
			}
			if tc.name == "shadowed builtin" && !hasBoundary(result, "shadowed_guard") {
				t.Fatalf("shadowed builtin not marked: %+v", result)
			}
		})
	}
}

func TestFlowPermittedSinkAndUnknownEdges(t *testing.T) {
	allowed := investigate(t, map[string]string{"main.lua": "Allow(Source())\n"}, Target{Path: "main.lua"})
	if len(allowed.Findings) != 0 {
		t.Fatalf("allowed argument reported: %+v", allowed)
	}
	alias := investigate(t, map[string]string{"main.lua": "local f=Source\nlocal x=f()\nUse(x)\n"}, Target{Path: "main.lua"})
	if len(alias.Findings) != 0 || !hasBoundary(alias, "unknown_argument_constraint") {
		t.Fatalf("unknown alias treated as known/clean: %+v", alias)
	}
	ambiguous := investigate(t, map[string]string{
		"a.lua":    "function Wrap() return Source() end\n",
		"b.lua":    "function Wrap() return nil end\n",
		"main.lua": "Use(Wrap())\n",
	}, Target{Path: "main.lua"})
	if len(ambiguous.Findings) != 0 || !hasBoundary(ambiguous, "ambiguous_call") {
		t.Fatalf("ambiguous global chosen: %+v", ambiguous)
	}
	scoped := investigate(t, map[string]string{
		"main.lua": "do\n local Wrap=function() return Source() end\nend\nUse(Wrap())\n",
	}, Target{Path: "main.lua"})
	if len(scoped.Findings) != 0 || !hasBoundary(scoped, "ambiguous_call") {
		t.Fatalf("block-local function escaped lexical scope: %+v", scoped)
	}
}

func TestFlowLexicalShadowAndUnknownSideEffects(t *testing.T) {
	block := investigate(t, map[string]string{"main.lua": "local x=Source()\ndo\n local x=0\nend\nUse(x)\n"}, Target{Path: "main.lua"})
	if len(block.Findings) != 1 {
		t.Fatalf("block local overwrote outer secret: %+v", block)
	}
	branch := investigate(t, map[string]string{"main.lua": "local x=Source()\nif true then\n local x=0\nend\nUse(x)\n"}, Target{Path: "main.lua"})
	if len(branch.Findings) == 0 {
		t.Fatalf("branch local overwrote outer secret: %+v", branch)
	}
	shadow := investigate(t, map[string]string{"main.lua": "local Use=function(x) return nil end\nUse(Source())\n"}, Target{Path: "main.lua"})
	if len(shadow.Findings) != 0 {
		t.Fatalf("local sink name used global API rule: %+v", shadow)
	}
	parameter := investigate(t, map[string]string{"main.lua": "function Demo(Use)\n Use(Source())\nend\n"}, Target{Path: "main.lua", Symbol: "Demo"})
	if len(parameter.Findings) != 0 || !hasBoundary(parameter, "shadowed_call") {
		t.Fatalf("parameter name used global API rule: %+v", parameter)
	}
	sideEffect := investigate(t, map[string]string{"main.lua": "function Demo()\n local x=Source()\n if issecretvalue(x) then return end\n Unknown()\n Use(x)\nend\n"}, Target{Path: "main.lua", Symbol: "Demo"})
	if len(sideEffect.Findings) != 0 || !hasBoundary(sideEffect, "unknown_side_effect") {
		t.Fatalf("unknown call left guard silently valid: %+v", sideEffect)
	}
}

func TestFlowInputAndPathBudget(t *testing.T) {
	_, err := Analyze(context.Background(), Request{Files: map[string][]byte{"main.lua": []byte(strings.Repeat("x", (1<<20)+1))}, Target: Target{Path: "main.lua"}, Rules: testRules()})
	if err == nil {
		t.Fatal("oversized source entered parser")
	}
	result, err := Analyze(context.Background(), Request{Files: map[string][]byte{"main.lua": []byte("local x=Source()\nUse(x)\n")}, Target: Target{Path: "main.lua"}, Rules: testRules(), Budget: Budget{MaxPaths: 1}})
	if err != nil || !result.Truncated || !hasBoundary(result, "path_budget") {
		t.Fatalf("path budget invisible: %+v %v", result, err)
	}
}

func hasBoundary(result Result, code string) bool {
	for _, item := range result.Boundaries {
		if item.Code == code {
			return true
		}
	}
	return false
}
