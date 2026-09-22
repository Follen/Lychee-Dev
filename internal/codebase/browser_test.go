package codebase

import (
	"bytes"
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func sourceFixture(t *testing.T) (*Browser, selection.SourcePin) {
	t.Helper()
	if _, err := exec.LookPath("git"); err != nil {
		t.Fatal("Git required for source tests")
	}
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	b := OpenBrowser(store)
	dir := b.mirror("wow-ui-source")
	if _, err := gitBytes(ctx, "", 4096, "init", "--bare", dir); err != nil {
		t.Fatal(err)
	}
	// Construct raw Git objects, with no checkout or platform newline conversion.
	run := func(input string, args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"--git-dir=" + dir}, args...)...)
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=Test", "GIT_AUTHOR_EMAIL=test@example.invalid", "GIT_COMMITTER_NAME=Test", "GIT_COMMITTER_EMAIL=test@example.invalid")
		cmd.Stdin = strings.NewReader(input)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("%v: %s", err, out)
		}
		return strings.TrimSpace(string(out))
	}
	blob := run("one\r\n世界\r\nlast", "hash-object", "-w", "--stdin")
	link := run("file.lua", "hash-object", "-w", "--stdin")
	tree := run("100644 blob "+blob+"\tfile.lua\n120000 blob "+link+"\tlink.lua\n", "mktree")
	commit := run("fixture", "commit-tree", tree)
	run("", "update-ref", "refs/heads/live", commit)
	return b, selection.SourcePin{Repository: "wow-ui-source", Product: "retail", RequestedRef: "refs/heads/live", ExactCommit: commit, ParserRevision: ParserRevision}
}

func TestReadFixedSpanPreservesBytes(t *testing.T) {
	b, pin := sourceFixture(t)
	result, err := b.ReadSpan(context.Background(), pin, SpanQuery{Path: "file.lua", FirstLine: 2, LineCount: 1})
	if err != nil {
		t.Fatal(err)
	}
	if result.Text != "世界\r\n" || result.FirstLine != 2 || result.LastLine != 2 || result.TotalLines != 3 || result.Commit != pin.ExactCommit {
		t.Fatalf("%+v", result)
	}
	data, err := b.store.ReadBlob(context.Background(), result.Blob, maxSourceBytes)
	if err != nil || !bytes.Equal(data, []byte("one\r\n世界\r\nlast")) {
		t.Fatalf("original changed: %q %v", data, err)
	}
	// Source inspection ignores the moving branch entirely.
	if _, err := gitBytes(context.Background(), b.mirror(pin.Repository), 1024, "update-ref", "-d", "refs/heads/live"); err != nil {
		t.Fatal(err)
	}
	result, err = b.ReadSpan(context.Background(), pin, SpanQuery{Path: "file.lua", FirstLine: 3, LineCount: 20})
	if err != nil || result.Text != "last" {
		t.Fatalf("%+v %v", result, err)
	}
}

func TestRejectUnsafePathsAndSymlinks(t *testing.T) {
	b, pin := sourceFixture(t)
	for _, name := range []string{"../file.lua", "/file.lua", `C:\file.lua`, "a/../file.lua", ":(glob)*", "file.lua\n", "missing.lua", "link.lua"} {
		if _, err := b.ReadSpan(context.Background(), pin, SpanQuery{Path: name, FirstLine: 1, LineCount: 1}); err == nil {
			t.Fatalf("accepted %q", name)
		}
	}
	for _, q := range []SpanQuery{{"file.lua", 0, 1}, {"file.lua", 4, 1}, {"file.lua", 1, 2001}} {
		if _, err := b.ReadSpan(context.Background(), pin, q); err == nil {
			t.Fatalf("accepted %+v", q)
		}
	}
}

func TestGitTransportBudgetAndCancellation(t *testing.T) {
	b, pin := sourceFixture(t)
	if _, err := gitBytes(context.Background(), b.mirror(pin.Repository), 1, "cat-file", "commit", pin.ExactCommit); err == nil || !strings.Contains(err.Error(), "output_limit") {
		t.Fatalf("budget: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := b.ReadSpan(ctx, pin, SpanQuery{"file.lua", 1, 1}); err == nil {
		t.Fatal("cancel ignored")
	}
	t.Setenv("GIT_DIR", filepath.Join(t.TempDir(), "unrelated"))
	if _, err := b.ReadSpan(context.Background(), pin, SpanQuery{"file.lua", 1, 1}); err != nil {
		t.Fatalf("inherited routing: %v", err)
	}
}

func TestPrepareRejectsAmbiguousRefWithoutNetwork(t *testing.T) {
	b, _ := sourceFixture(t)
	for _, reference := range []string{"main", "latest", "--upload-pack=bad", "refs/heads/main:other", "refs/tags/a..b"} {
		if _, err := b.PrepareSource(context.Background(), "wow-ui-source", "retail", reference); err == nil {
			t.Fatalf("accepted %q", reference)
		}
	}
}
