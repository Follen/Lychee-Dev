package codebase

import (
	"context"
	"os"
	"os/exec"
	"sort"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

func testCommit(t *testing.T, b *Browser, base selection.SourcePin, files map[string]string, message string) selection.SourcePin {
	t.Helper()
	run := func(input string, args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"--git-dir=" + b.mirror(base.Repository)}, args...)...)
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=CompareTest",
			"GIT_AUTHOR_EMAIL=compare@example.invalid",
			"GIT_COMMITTER_NAME=CompareTest",
			"GIT_COMMITTER_EMAIL=compare@example.invalid",
		)
		cmd.Stdin = strings.NewReader(input)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
		return strings.TrimSpace(string(out))
	}

	var buildTree func(map[string]string) string
	buildTree = func(contents map[string]string) string {
		paths := make([]string, 0, len(contents))
		for name := range contents {
			paths = append(paths, name)
		}
		sort.Strings(paths)
		entries := []string{}
		children := map[string]map[string]string{}
		for _, name := range paths {
			first, rest, nested := strings.Cut(name, "/")
			if nested {
				if children[first] == nil {
					children[first] = map[string]string{}
				}
				children[first][rest] = contents[name]
				continue
			}
			blob := run(contents[name], "hash-object", "-w", "--stdin")
			entries = append(entries, "100644 blob "+blob+"\t"+name+"\n")
		}
		dirs := []string{}
		for name := range children {
			dirs = append(dirs, name)
		}
		sort.Strings(dirs)
		for _, name := range dirs {
			entries = append(entries, "040000 tree "+buildTree(children[name])+"\t"+name+"\n")
		}
		return run(strings.Join(entries, ""), "mktree")
	}
	tree := buildTree(files)
	commit := run(message+"\n", "commit-tree", tree)
	base.ExactCommit = commit
	return base
}

func indexedComparePair(t *testing.T, before, after map[string]string) (*Browser, selection.SourcePin, selection.SourcePin) {
	t.Helper()
	b, seed := sourceFixture(t)
	from := testCommit(t, b, seed, before, "before")
	to := testCommit(t, b, seed, after, "after")
	ctx := context.Background()
	if _, err := b.IndexSource(ctx, from); err != nil {
		t.Fatalf("index before: %v", err)
	}
	if _, err := b.IndexSource(ctx, to); err != nil {
		t.Fatalf("index after: %v", err)
	}
	return b, from, to
}

func TestCompareTreesReportsDocumentAndDeclarationChanges(t *testing.T) {
	before := map[string]string{
		"changed.lua":     "function Changed(value)\nend\n",
		"duplicate-a.lua": "function Shared()\nend\n",
		"duplicate-b.lua": "function Shared()\nend\n",
		"moved.lua":       "function Moved()\nend\n",
		"removed.lua":     "function Removed()\nend\n",
		"stable.lua":      "function Stable()\nend\n",
	}
	after := map[string]string{
		"added.lua":       "function Added()\nend\n",
		"changed.lua":     "function Changed(other)\nend\n",
		"duplicate-a.lua": "function Shared()\nend\n",
		"duplicate-b.lua": "function Shared(argument)\nend\n",
		"moved.lua":       "\n\nfunction Moved()\nend\n",
		"stable.lua":      "function Stable()\nend\n",
	}
	b, from, to := indexedComparePair(t, before, after)
	result, err := b.CompareTrees(context.Background(), SourcePair{From: from, To: to}, ChangeQuery{Limit: 20})
	if err != nil {
		t.Fatal(err)
	}

	if result.DocumentChanges != 5 || len(result.Documents) != 5 {
		t.Fatalf("document totals: %+v", result)
	}
	wantPaths := []string{"added.lua", "changed.lua", "duplicate-b.lua", "moved.lua", "removed.lua"}
	for i, change := range result.Documents {
		if change.Path != wantPaths[i] {
			t.Fatalf("documents are not sorted: got %q at %d", change.Path, i)
		}
	}
	wantStatuses := map[string]string{
		"added.lua":       "added",
		"changed.lua":     "changed",
		"duplicate-b.lua": "changed",
		"moved.lua":       "changed",
		"removed.lua":     "removed",
	}
	for _, change := range result.Documents {
		if change.Status != wantStatuses[change.Path] {
			t.Fatalf("document %s status: got %q", change.Path, change.Status)
		}
	}

	if result.DeclarationChanges != 5 || len(result.Declarations) != 5 {
		t.Fatalf("declaration totals: %+v", result)
	}
	wantNames := []string{"Added", "Changed", "Moved", "Removed", "Shared"}
	for i, change := range result.Declarations {
		if change.Name != wantNames[i] || change.Category != "function" {
			t.Fatalf("declarations are not stably sorted: got %+v at %d", change, i)
		}
	}
	wantDeclarationStatuses := map[string]string{
		"Added":   "added",
		"Changed": "changed",
		"Moved":   "changed",
		"Removed": "removed",
		"Shared":  "changed",
	}
	for _, change := range result.Declarations {
		if change.Status != wantDeclarationStatuses[change.Name] {
			t.Fatalf("declaration %s status: got %q", change.Name, change.Status)
		}
	}

	shared := result.Declarations[4]
	if len(shared.Before) != 2 || len(shared.After) != 2 {
		t.Fatalf("duplicate declaration sites were lost: %+v", shared)
	}
	if shared.Before[0].Path != "duplicate-a.lua" || shared.Before[1].Path != "duplicate-b.lua" ||
		shared.After[0].Path != "duplicate-a.lua" || shared.After[1].Path != "duplicate-b.lua" {
		t.Fatalf("duplicate declaration order: %+v", shared)
	}
	if shared.Before[1].Signature != "function Shared()" || shared.After[1].Signature != "function Shared(argument)" {
		t.Fatalf("duplicate declaration signatures: %+v", shared)
	}

	moved := result.Declarations[2]
	if len(moved.Before) != 1 || len(moved.After) != 1 || moved.Before[0].Line != 1 || moved.After[0].Line != 3 ||
		moved.Before[0].Signature != moved.After[0].Signature {
		t.Fatalf("line-only movement was not retained as a position change: %+v", moved)
	}
	if result.Truncated {
		t.Fatal("unlimited result was marked truncated")
	}
}

func TestCompareTreesLimitIsPerCategoryAndCountsAllChanges(t *testing.T) {
	before := map[string]string{
		"a.lua": "function OldA()\nend\n",
		"b.lua": "function OldB()\nend\n",
		"c.lua": "function OldC()\nend\n",
	}
	after := map[string]string{
		"a.lua": "function NewA()\nend\n",
		"b.lua": "function NewB()\nend\n",
		"c.lua": "function NewC()\nend\n",
		"d.lua": "function NewD()\nend\n",
	}
	b, from, to := indexedComparePair(t, before, after)
	result, err := b.CompareTrees(context.Background(), SourcePair{From: from, To: to}, ChangeQuery{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Documents) != 1 || len(result.Declarations) != 1 ||
		result.DocumentChanges != 4 || result.DeclarationChanges != 7 || !result.Truncated {
		t.Fatalf("limit/counts: %+v", result)
	}
}

func TestCompareTreesSameCommitHasNoDifferences(t *testing.T) {
	files := map[string]string{"same.lua": "function Same()\nend\n"}
	b, from, _ := indexedComparePair(t, files, files)
	result, err := b.CompareTrees(context.Background(), SourcePair{From: from, To: from}, ChangeQuery{Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if result.DocumentChanges != 0 || result.DeclarationChanges != 0 || len(result.Documents) != 0 ||
		len(result.Declarations) != 0 || result.Truncated || result.From.Commit != from.ExactCommit || result.To.Commit != from.ExactCommit {
		t.Fatalf("same commit changed: %+v", result)
	}
}

func TestCompareTreesRejectsMissingIncompleteAndIncompatibleIndexes(t *testing.T) {
	ctx := context.Background()
	b, seed := sourceFixture(t)
	valid := testCommit(t, b, seed, map[string]string{"valid.lua": "function Valid()\nend\n"}, "valid")
	if _, err := b.CompareTrees(ctx, SourcePair{From: valid, To: valid}, ChangeQuery{Limit: 1}); err == nil || !strings.Contains(err.Error(), "index_not_ready") {
		t.Fatalf("missing index was accepted: %v", err)
	}

	if _, err := b.IndexSource(ctx, seed); err != nil {
		t.Fatalf("index incomplete fixture: %v", err)
	}
	if _, err := b.CompareTrees(ctx, SourcePair{From: seed, To: seed}, ChangeQuery{Limit: 1}); err == nil || !strings.Contains(err.Error(), "incomplete_index") {
		t.Fatalf("incomplete index was accepted: %v", err)
	}

	otherRepository := valid
	otherRepository.Repository = "other-repository"
	if _, err := b.CompareTrees(ctx, SourcePair{From: valid, To: otherRepository}, ChangeQuery{Limit: 1}); err == nil || err.Error() != "codebase.incompatible_comparison" {
		t.Fatalf("repository mismatch was accepted: %v", err)
	}
	otherParser := valid
	otherParser.ParserRevision = "different-parser"
	if _, err := b.CompareTrees(ctx, SourcePair{From: valid, To: otherParser}, ChangeQuery{Limit: 1}); err == nil || err.Error() != "codebase.incompatible_comparison" {
		t.Fatalf("parser mismatch was accepted: %v", err)
	}
}
