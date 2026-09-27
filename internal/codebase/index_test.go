package codebase

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestIndexRetainsSyntaxDiagnostics(t *testing.T) {
	b, pin := sourceFixture(t)
	summary, err := b.IndexSource(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	if summary.Complete || summary.Diagnostics != 1 || summary.Documents != 1 {
		t.Fatalf("%+v", summary)
	}
	result, err := b.FindSymbols(context.Background(), pin, "missing", 10)
	if err != nil || result.Index.Complete || len(result.Matches) != 0 {
		t.Fatalf("%+v %v", result, err)
	}
}

func TestFixtureIndexRejectsChangedContentAtSamePath(t *testing.T) {
	b, seed := sourceFixture(t)
	root := t.TempDir()
	name := filepath.Join(root, "Addon.lua")
	if err := os.WriteFile(name, []byte("function One() end\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	pin, err := FixtureSourcePin(seed.Repository, seed.Product, root)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := b.IndexFixture(context.Background(), pin, root); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(name, []byte("function Two() end\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := b.IndexFixture(context.Background(), pin, root); err == nil || !strings.Contains(err.Error(), "fixture_changed") {
		t.Fatalf("stale fixture map reused: %v", err)
	}
}

func TestIndexQueriesPinnedSyntaxAndReopens(t *testing.T) {
	b, pin := sourceFixture(t)
	run := func(input string, args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"-c", "user.name=Test", "-c", "user.email=test@example.invalid", "--git-dir=" + b.mirror(pin.Repository)}, args...)...)
		cmd.Stdin = strings.NewReader(input)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("%v: %s", err, out)
		}
		return strings.TrimSpace(string(out))
	}
	blob := run("function Handler(self)\n  self:RegisterEvent(\"EVENT_A\")\n  Called()\nend\n", "hash-object", "-w", "--stdin")
	tree := run("100644 blob "+blob+"\tfixture.lua\n", "mktree")
	pin.ExactCommit = run("indexed fixture", "commit-tree", tree)
	ctx := context.Background()
	summary, err := b.IndexSource(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	if !summary.Complete || summary.Documents != 1 || summary.Declarations != 1 {
		t.Fatalf("%+v", summary)
	}
	for _, name := range []string{"Handler", "Called", "EVENT_A"} {
		result, err := OpenBrowser(b.store).FindSymbols(ctx, pin, name, 20)
		if err != nil || len(result.Matches) == 0 || result.Index.Commit != pin.ExactCommit {
			t.Fatalf("%s: %+v %v", name, result, err)
		}
	}
	limited, err := b.FindSymbols(ctx, pin, "Handler", 1)
	if err != nil || len(limited.Matches) != 1 || !limited.Truncated {
		t.Fatalf("%+v %v", limited, err)
	}
	// Repeat preparation opens the immutable completed index, without mutation.
	stat, err := os.Stat(b.indexPath(pin))
	if err != nil {
		t.Fatal(err)
	}
	repeated, err := b.IndexSource(ctx, pin)
	if err != nil || !reflect.DeepEqual(repeated, summary) {
		t.Fatalf("%+v %v", repeated, err)
	}
	again, _ := os.Stat(b.indexPath(pin))
	if !again.ModTime().Equal(stat.ModTime()) {
		t.Fatal("index was rewritten")
	}
	pin.ExactCommit = strings.Repeat("a", 40)
	if _, err := b.FindSymbols(ctx, pin, "Handler", 20); err == nil {
		t.Fatal("different commit reused index")
	}
}

func TestCancelledIndexIsNotPublished(t *testing.T) {
	b, pin := sourceFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := b.IndexSource(ctx, pin); err == nil {
		t.Fatal("cancel ignored")
	}
	if _, err := os.Stat(b.indexPath(pin)); !os.IsNotExist(err) {
		t.Fatalf("partial index: %v", err)
	}
}
