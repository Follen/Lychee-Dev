package codebase

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestContextDepthExactLoadAndSnippetContinuation(t *testing.T) {
	ctx := context.Background()
	fixture := t.TempDir()
	for name, body := range map[string]string{
		"A/A.toc":    "Main.lua\n",
		"B/B.toc":    "Main.lua\n",
		"A/Main.lua": "function Foo()\n  Bar()\nend\nfunction Bar() Baz() end\nfunction Baz() end\n",
		"B/Main.lua": "function Other() end\n",
	} {
		full := filepath.Join(fixture, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(full, []byte(body), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	q := ContextQuery{Symbol: "Foo", Depth: 1, Limit: 2, MaxBytes: 4096, MaxLines: 1}
	first, err := b.Context(ctx, indexed.Snapshot.ID, indexed.Pin, q, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if len(first.LoadEvidence) != 1 || first.LoadEvidence[0].Path != "A/A.toc" {
		t.Fatalf("exact load = %+v", first.LoadEvidence)
	}
	if len(first.Snippets) != 1 || first.NextCursor == "" {
		t.Fatalf("first page = %+v", first)
	}
	q.Cursor = first.NextCursor
	seenLines := map[int]bool{first.Snippets[0].FirstLine: true}
	seenRelated := false
	for page := 0; page < 10 && q.Cursor != ""; page++ {
		part, err := b.Context(ctx, indexed.Snapshot.ID, indexed.Pin, q, ResearchOptions{})
		if err != nil {
			t.Fatal(err)
		}
		for _, snippet := range part.Snippets {
			if snippet.Basis == "definition" {
				seenLines[snippet.FirstLine] = true
			}
			if snippet.Basis == "relation:call" {
				seenRelated = true
			}
		}
		q.Cursor = part.NextCursor
	}
	if !seenLines[1] || !seenLines[2] || !seenLines[3] || !seenRelated {
		t.Fatalf("continuation skipped definition or related snippet: lines=%v related=%v", seenLines, seenRelated)
	}
	q = ContextQuery{Symbol: "Foo", Depth: 0, Limit: 2, MaxBytes: 4096, MaxLines: 20}
	focus, err := b.Context(ctx, indexed.Snapshot.ID, indexed.Pin, q, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if len(focus.Relations) != 0 || len(focus.Snippets) != 1 || focus.Snippets[0].Basis != "definition" {
		t.Fatalf("depth zero = %+v", focus)
	}
	deep, err := b.Context(ctx, indexed.Snapshot.ID, indexed.Pin, ContextQuery{Symbol: "Foo", Depth: 4, Limit: 20, MaxBytes: 16384, MaxLines: 100}, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	depthEvidence := false
	for _, snippet := range deep.Snippets {
		if snippet.Basis == "static-depth:2" {
			depthEvidence = true
		}
	}
	if !depthEvidence {
		t.Fatalf("depth four did not expand: %+v", deep.Snippets)
	}
}

func TestLoadedPathRejectsSuffixCollisionAndTraversal(t *testing.T) {
	if got := loadedPath("A/A.toc", "Main.lua"); got != "A/Main.lua" {
		t.Fatal(got)
	}
	if got := loadedPath("B/B.toc", "Main.lua"); got != "B/Main.lua" {
		t.Fatal(got)
	}
	if got := loadedPath("A/A.toc", "../../Main.lua"); got != "" {
		t.Fatal(got)
	}
}

func TestContextFlowUsesVersionedGeneratedMetadata(t *testing.T) {
	ctx := context.Background()
	fixture := t.TempDir()
	api := filepath.Join(fixture, "Interface", "AddOns", "Blizzard_APIDocumentationGenerated", "UnitDocumentation.lua")
	if err := os.MkdirAll(filepath.Dir(api), 0o755); err != nil {
		t.Fatal(err)
	}
	metadata := `local Unit = { Name="Unit", Type="System", Functions={
 {Name="UnitHealth",Type="Function",SecretReturns=true,Returns={{Name="health",Type="number"}}},
 {Name="Danger",Type="Function",SecretArguments="AllowedWhenUntainted",Arguments={{Name="value",Type="number"}}}
}}; APIDocumentation:AddDocumentationTable(Unit)`
	if err := os.WriteFile(api, []byte(metadata), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(fixture, "Addon.lua"), []byte("function Foo()\n local health = Wrapper()\n Danger(health)\nend\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(fixture, "Helper.lua"), []byte("function Wrapper()\n return UnitHealth('player')\nend\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	indexed, err := IndexFixtureSource(ctx, store.Root(), "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	result, err := OpenBrowser(store).Context(ctx, indexed.Snapshot.ID, indexed.Pin, ContextQuery{Symbol: "Foo", Depth: 1, Limit: 10, MaxBytes: 4096, MaxLines: 40}, ResearchOptions{Flow: true})
	if err != nil {
		t.Fatal(err)
	}
	if result.Flow == nil || result.Flow.RuleIdentity.APICommit != indexed.Pin.ExactCommit || result.Flow.RuleIdentity.MetadataDigest == "" || len(result.Flow.Findings) == 0 {
		t.Fatalf("flow result = %+v coverage = %+v", result.Flow, result.Coverage)
	}
	callee := false
	for _, snippet := range result.Snippets {
		if snippet.Path == "Helper.lua" && snippet.Basis == "candidate-callee:Wrapper" {
			callee = true
		}
	}
	if !callee {
		t.Fatalf("two-file wrapper missing from context: %+v", result.Snippets)
	}
	apiContext, err := OpenBrowser(store).Context(ctx, indexed.Snapshot.ID, indexed.Pin, ContextQuery{Symbol: "UnitHealth", Depth: 0, Limit: 5, MaxBytes: 4096, MaxLines: 40}, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	if apiContext.APIFact == nil || apiContext.EnvironmentManifest == nil || apiContext.APIFact.Raw["SecretReturns"] != true {
		t.Fatalf("API fact/identity missing: %+v", apiContext)
	}
	foundRaw := false
	for _, snippet := range apiContext.Snippets {
		if strings.Contains(snippet.Text, "SecretReturns=true") {
			foundRaw = true
		}
	}
	if !foundRaw {
		t.Fatalf("selected API declaration excerpt incomplete: %+v", apiContext.Snippets)
	}
}

func TestContextExcerptContinuesLongLineWithoutDroppingBytes(t *testing.T) {
	data := []byte("abcdefghijklmnop\n")
	line, offset := 1, 0
	joined := ""
	for n := 0; n < 10; n++ {
		part, _, nextLine, nextByte, done, err := contextExcerpt(data, line, 1, offset, 5, 1)
		if err != nil {
			t.Fatal(err)
		}
		joined += part
		if done {
			break
		}
		line, offset = nextLine, nextByte
	}
	if joined != "1: abcdefghijklmnop" {
		t.Fatalf("continuation dropped bytes: %q", joined)
	}
	exact, last, _, _, done, err := contextExcerpt([]byte("abc\n"), 1, 1, 0, len("1: abc"), 1)
	if err != nil || !done || last != 1 || exact != "1: abc" {
		t.Fatalf("exact boundary falsely truncated: text=%q last=%d done=%v err=%v", exact, last, done, err)
	}
}
