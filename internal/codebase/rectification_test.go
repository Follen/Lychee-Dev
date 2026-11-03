package codebase

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestExplicitClientEnvironmentIsValidated(t *testing.T) {
	ctx := context.Background()
	home, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, evidenceTree("120100"), "environment")
	if _, err := b.IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	save := func(p selection.SourcePin) string {
		t.Helper()
		set, err := vault.WriteMetadata(ctx, home, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
			return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &p})
		})
		if err != nil {
			t.Fatal(err)
		}
		return set.ID
	}
	same := save(pin)
	other := pin
	other.Product = "classic"
	different := save(other)
	for _, tc := range []struct {
		name, id  string
		wantError bool
	}{{"implicit", "", false}, {"same", same, false}, {"different", different, true}, {"absent", "PIN-absent", true}} {
		t.Run(tc.name, func(t *testing.T) {
			_, state, err := b.researchEnvironment(ctx, pin, tc.id)
			if (err != nil) != tc.wantError {
				t.Fatalf("state=%s err=%v", state, err)
			}
		})
	}
	third := pin
	third.Repository = "elvui"
	third.Product = "main"
	if _, state, err := b.researchEnvironment(ctx, third, same); err != nil || state != "ready" {
		t.Fatalf("third-party: %s %v", state, err)
	}
	if _, state, err := b.researchEnvironment(ctx, third, ""); err != nil || state != "missing-client-pin" {
		t.Fatalf("missing: %s %v", state, err)
	}
	if _, _, err := b.researchEnvironment(ctx, pin, different); !errors.Is(err, ErrInvalidQuery) {
		t.Fatalf("conflict category: %v", err)
	}
}

func TestRefsCursorBindsEnvironmentRevisionWithIdenticalAPIFacts(t *testing.T) {
	ctx := context.Background()
	home, b, seed := workspaceFixture(t)
	files := evidenceTree("120100")
	first := testCommit(t, b, seed, files, "first-api-revision")
	files["Unrelated.lua"] = "return 1\n"
	second := testCommit(t, b, first, files, "second-api-revision")
	save := func(pin selection.SourcePin) string {
		t.Helper()
		set, err := vault.WriteMetadata(ctx, home, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
			return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
		})
		if err != nil {
			t.Fatal(err)
		}
		return set.ID
	}
	firstID, secondID := save(first), save(second)
	root := t.TempDir()
	writeAddonFiles(t, root, map[string]string{"Addon.lua": "function Foo() return 42 end\nfunction Bar() return Foo() end\nfunction Baz() return Foo() end\n"})
	source, err := indexGitFixture(t, ctx, home, "elvui", "main", root)
	if err != nil {
		t.Fatal(err)
	}
	query := RelationQuery{Symbol: "Foo", Limit: 1}
	page, err := b.Relate(ctx, source.Snapshot.ID, source.Pin, query, ResearchOptions{EnvironmentSnapshot: firstID})
	if err != nil || page.NextCursor == "" {
		t.Fatalf("page=%+v err=%v", page, err)
	}
	query.Cursor = page.NextCursor
	if _, err := b.Relate(ctx, source.Snapshot.ID, source.Pin, query, ResearchOptions{EnvironmentSnapshot: firstID}); err != nil {
		t.Fatal(err)
	}
	if _, err := b.Relate(ctx, source.Snapshot.ID, source.Pin, query, ResearchOptions{EnvironmentSnapshot: secondID}); !errors.Is(err, ErrInvalidSearchCursor) {
		t.Fatalf("accepted different API revision: %v", err)
	}
}

func TestContextAggregatesNonTruncatedFlowCoverage(t *testing.T) {
	for _, tc := range []struct {
		name, file, source string
		complete           bool
	}{
		{"bounded", "Addon.lua", "function Foo() return 42 end\n", true},
		{"unknown", "Addon.lua", "function Foo() return UnknownCall() end\n", false},
		{"omitted", "Addon.lua", "function Foo() return 42 end\n--" + strings.Repeat("x", 1<<20) + "\n", false},
		{"unsupported", "Addon.xml", "<Ui><Frame name=\"Foo\"/></Ui>", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctx := context.Background()
			home, b, seed := workspaceFixture(t)
			files := evidenceTree("120100")
			files[tc.file] = tc.source
			pin := testCommit(t, b, seed, files, tc.name)
			if _, err := b.IndexSource(ctx, pin); err != nil {
				t.Fatal(err)
			}
			related, err := b.Relate(ctx, "fixed", pin, RelationQuery{Symbol: "Foo", Limit: 10}, ResearchOptions{})
			if err != nil {
				t.Fatal(err)
			}
			env, _, err := b.researchEnvironment(ctx, pin, "")
			if err != nil {
				t.Fatal(err)
			}
			cache, _, err := b.openIndex(ctx, pin)
			if err != nil {
				t.Fatal(err)
			}
			defer cache.Close()
			runtime := &luals.Runtime{Identity: luals.Identity{Version: "test", SHA256: "test"}}
			b.cachedSemantic(ctx, pin, cache.manifest.RecordsHash, env.Manifest, runtime, *related.Symbol, "both", func() ([]ResearchRelation, string) { return []ResearchRelation{}, "" })
			snapshot, err := vault.WriteMetadata(ctx, home, func(_ *vault.Store, m *vault.Metadata) (selection.PinnedSet, error) {
				return selection.OpenPinner(m).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
			})
			if err != nil {
				t.Fatal(err)
			}
			reading, err := ContextSource(ctx, home, snapshot.ID, ContextQuery{Symbol: "Foo", Limit: 10, MaxBytes: 8192}, ResearchOptions{Semantic: runtime, Flow: true})
			if err != nil {
				t.Fatal(err)
			}
			result := reading.Result
			if reading.Capture.Complete != result.Complete {
				t.Fatalf("capture contradicts body: %+v", reading.Capture)
			}
			if result.Truncated || result.Complete != tc.complete || (result.Coverage.State == "complete") != tc.complete {
				t.Fatalf("complete=%v truncated=%v coverage=%+v flow=%+v", result.Complete, result.Truncated, result.Coverage, result.Flow)
			}
		})
	}
}

func TestContextAndRefsCursorBindSemanticContext(t *testing.T) {
	ctx := context.Background()
	_, b, seed := workspaceFixture(t)
	files := evidenceTree("120100")
	files["Addon.lua"] = "function Foo()\n return 42\nend\nfunction Bar() return Foo() end\n"
	pin := testCommit(t, b, seed, files, "cursor")
	if _, err := b.IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	runtimes := []*luals.Runtime{}
	for _, label := range []string{"one", "two"} {
		release := t.TempDir()
		dir := filepath.Join(release, "payload", "tool", "luals")
		for _, name := range []string{"bin/lua-language-server.exe", "bin/main.lua", "main.lua", "LICENSE"} {
			p := filepath.Join(dir, name)
			if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(p, []byte("unused"), 0600); err != nil {
				t.Fatal(err)
			}
		}
		raw, _ := json.Marshal(luals.Pinned())
		if err := os.WriteFile(filepath.Join(dir, "runtime.json"), raw, 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(release, "release.json"), []byte(label), 0600); err != nil {
			t.Fatal(err)
		}
		runtime, err := luals.Open(ctx, release, func(context.Context, string) error { return nil })
		if err != nil {
			t.Fatal(err)
		}
		runtimes = append(runtimes, runtime)
	}
	related, err := b.Relate(ctx, "fixed", pin, RelationQuery{Symbol: "Foo", Limit: 10}, ResearchOptions{})
	if err != nil {
		t.Fatal(err)
	}
	env, _, err := b.researchEnvironment(ctx, pin, "")
	if err != nil {
		t.Fatal(err)
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	for _, runtime := range runtimes {
		b.cachedSemantic(ctx, pin, cache.manifest.RecordsHash, env.Manifest, runtime, *related.Symbol, "both", func() ([]ResearchRelation, string) {
			return []ResearchRelation{{Source: "Bar", Target: "Foo", Kind: "reference", Path: "Addon.lua", Line: 4, State: "resolved"}}, ""
		})
	}
	q := ContextQuery{Symbol: "Foo", Limit: 10, MaxLines: 1}
	first, err := b.Context(ctx, "fixed", pin, q, ResearchOptions{Semantic: runtimes[0]})
	if err != nil || first.NextCursor == "" {
		t.Fatalf("first=%+v %v", first, err)
	}
	q.Cursor = first.NextCursor
	if _, err := b.Context(ctx, "fixed", pin, q, ResearchOptions{Semantic: runtimes[0]}); err != nil {
		t.Fatal(err)
	}
	if _, err := b.Context(ctx, "fixed", pin, q, ResearchOptions{Semantic: runtimes[1]}); !errors.Is(err, ErrInvalidSearchCursor) {
		t.Fatalf("different context accepted: %v", err)
	}
	rq := RelationQuery{Symbol: "Foo", Limit: 1}
	page, err := b.Relate(ctx, "fixed", pin, rq, ResearchOptions{Semantic: runtimes[0]})
	if err != nil || page.NextCursor == "" {
		t.Fatalf("refs=%+v %v", page, err)
	}
	rq.Cursor = page.NextCursor
	if _, err := b.Relate(ctx, "fixed", pin, rq, ResearchOptions{Semantic: runtimes[0]}); err != nil {
		t.Fatal(err)
	}
	if _, err := b.Relate(ctx, "fixed", pin, rq, ResearchOptions{Semantic: runtimes[1]}); !errors.Is(err, ErrInvalidSearchCursor) {
		t.Fatalf("different refs context accepted: %v", err)
	}
}

func TestThirdPartyStaticValidationUsesIndependentClientEnvironment(t *testing.T) {
	ctx := context.Background()
	home, _, _ := workspaceFixture(t)
	apiRoot := t.TempDir()
	writeAddonFiles(t, apiRoot, evidenceTree("120100"))
	api, err := indexGitFixture(t, ctx, home, "wow-ui-source", "retail", apiRoot)
	if err != nil {
		t.Fatal(err)
	}
	addonRoot := t.TempDir()
	writeAddonFiles(t, addonRoot, map[string]string{"Addon.toc": "## Interface: 120100\n[Family]/Main.lua\nMissing.lua [AllowLoadGameType classic]\n", "Mainline/Main.lua": "C_Good.Do()\n"})
	source, err := indexGitFixture(t, ctx, home, "elvui", "main", addonRoot)
	if err != nil {
		t.Fatal(err)
	}
	input := AddonInput{Root: addonRoot, Manifest: "Addon.toc"}
	if _, err := AssessAddon(ctx, home, source.Snapshot.ID, input); !errors.Is(err, ErrInvalidQuery) {
		t.Fatalf("third-party missing environment accepted: %v", err)
	}
	result, err := AssessAddon(ctx, home, source.Snapshot.ID, input, ValidationOptions{EnvironmentSnapshot: api.Snapshot.ID})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Result.StaticValid || result.Result.Resolved != 1 || !result.Result.Load.LoadComplete || result.Result.Source != source.Pin || result.Result.Environment == nil || *result.Result.Environment != api.Pin || result.Result.Load.Context.Family != "Mainline" {
		t.Fatalf("third-party environment lost: %+v", result.Result)
	}
	if result.Capture.Provenance.Snapshot != source.Snapshot.ID || result.Capture.Complete != result.Result.Complete {
		t.Fatalf("capture=%+v", result.Capture)
	}
	matrixFile := filepath.Join(addonRoot, "matrix.json")
	raw, _ := json.Marshal(MatrixConfig{Path: ".", Targets: []MatrixTarget{{ID: "addon", TOC: "Addon.toc", Source: "elvui", Product: "main", Ref: source.Pin.ExactCommit}}})
	if err := os.WriteFile(matrixFile, raw, 0600); err != nil {
		t.Fatal(err)
	}
	resolve := func(context.Context, string, string, string) (selection.SourcePin, error) { return source.Pin, nil }
	matrix, err := ValidateSourceMatrix(ctx, home, matrixFile, resolve, ValidationOptions{EnvironmentSnapshot: api.Snapshot.ID})
	if err != nil {
		t.Fatal(err)
	}
	target := matrix.Result.Targets[0]
	if !target.Valid || target.ResolvedCommit != source.Pin.ExactCommit || target.Environment == nil || *target.Environment != api.Pin || target.CheckedLua != 1 || target.Coverage.Unresolved != 0 {
		t.Fatalf("matrix=%+v", target)
	}
	if _, err := AssessAddon(ctx, home, api.Snapshot.ID, input, ValidationOptions{EnvironmentSnapshot: source.Snapshot.ID}); err == nil {
		t.Fatal("non-client environment accepted")
	}
}

func TestRetiredForeverSourceTracksRemainReadableButNotSelectable(t *testing.T) {
	ctx := context.Background()
	_, b, seed := workspaceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"Addon.lua": "function Historical() end\n"}, "historical source")
	repo, err := LookupRepository("wow-ui-source")
	if err != nil {
		t.Fatal(err)
	}
	for _, product := range []string{"forever-asia-beta", "forever-cn-beta"} {
		if _, listed := repo.Tracks[product]; listed {
			t.Fatalf("retired track listed: %s", product)
		}
		if _, err := b.PrepareSource(ctx, repo.Key, product, pin.ExactCommit); !errors.Is(err, ErrUnknownProduct) {
			t.Fatalf("retired new selection %s: %v", product, err)
		}
		historical := pin
		historical.Product = product
		if _, err := b.IndexSource(ctx, historical); err != nil {
			t.Fatal(err)
		}
		span, err := b.ReadSpan(ctx, historical, SpanQuery{Path: "Addon.lua", FirstLine: 1, LineCount: 1})
		if err != nil || !strings.Contains(span.Text, "Historical") {
			t.Fatalf("history unreadable: %+v %v", span, err)
		}
	}
}
