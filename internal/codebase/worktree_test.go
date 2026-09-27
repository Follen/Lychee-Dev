package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestWorktreeTextAutoPreservesCommittedLFBytes(t *testing.T) {
	b, seed := sourceFixture(t)
	content := []byte("root = true\n[*]\nindent_style = space\nend_of_line = lf\n")
	pin := testCommit(t, b, seed, map[string]string{
		".gitattributes": "* text=auto\n*.sh eol=lf\n",
		".editorconfig":  string(content),
		"source.lua":     "function Sample() end\n",
	}, "text auto LF")
	lease, err := b.AcquireWorktree(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	actual, err := os.ReadFile(filepath.Join(lease.Path(), ".editorconfig"))
	if err != nil || !bytes.Equal(actual, content) {
		t.Fatalf("text=auto changed committed LF bytes: %q %v", actual, err)
	}
}

func TestWorktreeExplicitCRLFAttributeFailsClosed(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{
		".gitattributes": "*.lua text eol=crlf\n",
		"source.lua":     "function Sample() end\n",
	}, "explicit CRLF conversion")
	if _, err := b.AcquireWorktree(context.Background(), pin); err == nil || !errors.Is(err, ErrSourceIntegrity) || !strings.Contains(err.Error(), "content_mismatch") {
		t.Fatalf("attribute conversion bypassed exact blob check: %v", err)
	}
}

func TestWorktreeExactReuseAndDirtyDetection(t *testing.T) {
	b, seed := sourceFixture(t)
	one := testCommit(t, b, seed, map[string]string{"src/main.lua": "function Alpha() end\n", ".luarc.json": "{}\n"}, "worktree one")
	two := testCommit(t, b, seed, map[string]string{"src/main.lua": "function Beta() end\n", ".luarc.json": "{}\n"}, "worktree two")
	ctx := context.Background()
	first, err := b.AcquireWorktree(ctx, one)
	if err != nil {
		t.Fatal(err)
	}
	firstPath := first.Path()
	first.Close()
	second, err := b.AcquireWorktree(ctx, two)
	if err != nil {
		t.Fatal(err)
	}
	defer second.Close()
	if firstPath == second.Path() {
		t.Fatal("two commits shared a checkout")
	}
	again, err := b.AcquireWorktree(ctx, one)
	if err != nil {
		t.Fatal(err)
	}
	if again.Path() != firstPath {
		t.Fatal("fixed checkout was not reused")
	}
	again.Close()
	name := filepath.Join(firstPath, "src", "main.lua")
	if err := os.WriteFile(name, []byte("function Changed() end\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := b.AcquireWorktree(ctx, one); err == nil || !strings.Contains(err.Error(), "content_mismatch") {
		t.Fatalf("dirty source accepted: %v", err)
	}
	if err := os.WriteFile(name, []byte("function Alpha() end\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(firstPath, "injected.lua"), []byte("bad"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := b.AcquireWorktree(ctx, one); err == nil || !strings.Contains(err.Error(), "extra_or_link") {
		t.Fatalf("extra input accepted: %v", err)
	}
}

func TestWorktreeRecoversVerifiedOrphan(t *testing.T) {
	b, seed := sourceFixture(t)
	one := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "orphan one")
	two := testCommit(t, b, seed, map[string]string{"two.lua": "Two = true\n"}, "orphan two")
	lease, err := b.AcquireWorktree(context.Background(), one)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
	if err := os.Remove(b.worktreeManifestPath(one)); err != nil {
		t.Fatal(err)
	}
	other, err := b.AcquireWorktree(context.Background(), two)
	if err != nil {
		t.Fatal(err)
	}
	other.Close()
	if _, err := os.Stat(b.worktreeManifestPath(one)); err != nil {
		t.Fatalf("orphan not recovered/accounted: %v", err)
	}
	if _, err := b.verifyWorktree(context.Background(), one, b.worktreePath(one)); err != nil {
		t.Fatal(err)
	}
}

func TestWorktreePreservesModifiedOrphan(t *testing.T) {
	b, seed := sourceFixture(t)
	one := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "modified orphan one")
	two := testCommit(t, b, seed, map[string]string{"two.lua": "Two = true\n"}, "modified orphan two")
	lease, err := b.AcquireWorktree(context.Background(), one)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
	if err := os.Remove(b.worktreeManifestPath(one)); err != nil {
		t.Fatal(err)
	}
	modified := filepath.Join(b.worktreePath(one), "one.lua")
	if err := os.WriteFile(modified, []byte("UserChanged = true\n"), 0600); err != nil {
		t.Fatal(err)
	}
	other, err := b.AcquireWorktree(context.Background(), two)
	if err != nil {
		t.Fatalf("non-target orphan blocked clean checkout: %v", err)
	}
	other.Close()
	if _, err := b.AcquireWorktree(context.Background(), one); err == nil || !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("selected dirty orphan accepted: %v", err)
	}
	data, err := os.ReadFile(modified)
	if err != nil || string(data) != "UserChanged = true\n" {
		t.Fatalf("modified orphan was deleted: %v", err)
	}
}

func TestDirtyOrphanInOtherRepositoryIsPreservedAndCharged(t *testing.T) {
	b, seed := sourceFixture(t)
	ctx := context.Background()
	waBase := seed
	waBase.Repository, waBase.Product = "weakauras", "main"
	if _, err := gitBytes(ctx, "", 4096, "init", "--bare", b.mirror(waBase.Repository)); err != nil {
		t.Fatal(err)
	}
	wa := testCommit(t, b, waBase, map[string]string{".editorconfig": "root = true\n"}, "WA orphan")
	retail := testCommit(t, b, seed, map[string]string{"Retail.lua": "Retail = true\n"}, "Retail clean")
	waLease, err := b.AcquireWorktree(ctx, wa)
	if err != nil {
		t.Fatal(err)
	}
	waLease.Close()
	if err := os.Remove(b.worktreeManifestPath(wa)); err != nil {
		t.Fatal(err)
	}
	modified := filepath.Join(b.worktreePath(wa), ".editorconfig")
	if err := os.WriteFile(modified, []byte("root = false\n"), 0600); err != nil {
		t.Fatal(err)
	}
	retailLease, err := b.AcquireWorktree(ctx, retail)
	if err != nil {
		t.Fatalf("WA orphan blocked Retail: %v", err)
	}
	retailLease.Close()
	if _, err := b.AcquireWorktree(ctx, wa); err == nil || !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("selected WA orphan accepted: %v", err)
	}
	pruned, err := PruneSourceWorktrees(ctx, b.store.Root(), 0)
	if err != nil {
		t.Fatal(err)
	}
	if pruned.Complete || pruned.AfterBytes < int64(len("root = false\n")) || pruned.BeforeBytes <= pruned.AfterBytes {
		t.Fatalf("orphan bytes not retained in prune accounting: %+v", pruned)
	}
	data, err := os.ReadFile(modified)
	if err != nil || string(data) != "root = false\n" {
		t.Fatalf("other repository orphan changed: %q %v", data, err)
	}
}

func TestPruneStaleManifestAndGitRegistration(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "stale worktree")
	lease, err := b.AcquireWorktree(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
	if err := os.RemoveAll(b.worktreePath(pin)); err != nil {
		t.Fatal(err)
	}
	result, err := PruneSourceWorktrees(context.Background(), b.store.Root(), 0)
	if err != nil || !result.Complete || len(result.Removed) != 1 {
		t.Fatalf("stale cleanup failed: %+v %v", result, err)
	}
	if _, err := os.Stat(b.worktreeManifestPath(pin)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("stale manifest remains: %v", err)
	}
}

func TestWorktreeManifestConflictPreservesAccounting(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "manifest conflict")
	lease, err := b.AcquireWorktree(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
	path := b.worktreeManifestPath(pin)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var manifest worktreeManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.Tree = strings.Repeat("0", 40)
	conflict, _ := json.Marshal(manifest)
	if err := os.WriteFile(path, conflict, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := b.AcquireWorktree(context.Background(), pin); err == nil || !strings.Contains(err.Error(), "manifest_conflict") || !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("conflicting manifest accepted: %v", err)
	}
	after, err := os.ReadFile(path)
	if err != nil || string(after) != string(conflict) {
		t.Fatalf("manifest removed/replaced during conflict: %v", err)
	}
}

func TestWorktreeRejectsForeignGitdirBeforeGit(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "foreign gitdir")
	lease, err := b.AcquireWorktree(context.Background(), pin)
	if err != nil {
		t.Fatal(err)
	}
	lease.Close()
	gitfile := filepath.Join(b.worktreePath(pin), ".git")
	if err := os.WriteFile(gitfile, []byte("gitdir: "+filepath.ToSlash(t.TempDir())+"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := b.AcquireWorktree(context.Background(), pin); err == nil || !strings.Contains(err.Error(), "identity_mismatch") || !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("foreign gitdir accepted: %v", err)
	}
}

func TestWorktreeCapacitySerializesDifferentCommits(t *testing.T) {
	b, seed := sourceFixture(t)
	one := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "capacity one")
	two := testCommit(t, b, seed, map[string]string{"two.lua": "Two = true\n"}, "capacity two")
	three := testCommit(t, b, seed, map[string]string{"three.lua": "Three = true\n"}, "capacity three")
	active, err := b.AcquireWorktree(context.Background(), one)
	if err != nil {
		t.Fatal(err)
	}
	defer active.Close()
	// A manifest is only a record, not capacity evidence. Even an inflated
	// record for an active checkout must not displace the actual-byte budget.
	path := b.worktreeManifestPath(one)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var manifest worktreeManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.Bytes = defaultSourceWorktreeBudget - 1
	staged, _ := json.Marshal(manifest)
	if err := os.WriteFile(path, staged, 0600); err != nil {
		t.Fatal(err)
	}
	results := make(chan error, 2)
	for _, pin := range []selection.SourcePin{two, three} {
		go func(pin selection.SourcePin) {
			lease, err := b.AcquireWorktree(context.Background(), pin)
			if err == nil {
				err = lease.Close()
			}
			results <- err
		}(pin)
	}
	for range 2 {
		if err := <-results; err != nil {
			t.Fatalf("concurrent different-commit checkout failed: %v", err)
		}
	}
	for _, pin := range []selection.SourcePin{two, three} {
		if _, err := b.verifyWorktree(context.Background(), pin, b.worktreePath(pin)); err != nil {
			t.Fatalf("concurrent checkout not verified: %s %v", pin.ExactCommit, err)
		}
	}
	actual, err := sourcePathBytes(context.Background(), filepath.Join(b.store.Root(), "source", "v1", "worktrees"))
	if err != nil || actual >= defaultSourceWorktreeBudget {
		t.Fatalf("actual checkout budget miscounted: %d %v", actual, err)
	}
}

func TestPruneCancellationWhileCapacityBusy(t *testing.T) {
	b, _ := sourceFixture(t)
	lease, err := vault.AcquireLease(context.Background(), filepath.Join(b.store.Root(), "locks"), "source:v1:worktree-capacity")
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 80*time.Millisecond)
	defer cancel()
	_, err = PruneSourceWorktrees(ctx, b.store.Root(), 0)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("prune cancellation lost: %v", err)
	}
}

func TestWorktreeRejectsCheckoutFilterBeforeExecuting(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"one.lua": "One = true\n"}, "unsafe filter")
	marker := filepath.Join(t.TempDir(), "filter-ran")
	// The executable itself is irrelevant; any non-allowlisted filter setting
	// must be rejected before the checkout operation can invoke it.
	_, err := gitBytes(context.Background(), b.mirror(pin.Repository), 1024, "config", "--local", "filter.untrusted.smudge", "powershell -NoProfile -Command New-Item "+marker)
	if err != nil {
		t.Fatal(err)
	}
	_, err = b.AcquireWorktree(context.Background(), pin)
	if err == nil || !strings.Contains(err.Error(), "worktree_untrusted_git_config") || !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("unsafe filter config accepted: %v", err)
	}
	if _, err := os.Stat(marker); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("filter executed: %v", err)
	}
	if _, err := os.Stat(b.worktreePath(pin)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("checkout created under unsafe config: %v", err)
	}
}

func TestWorktreeConcurrentSameCommit(t *testing.T) {
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"src/main.lua": "function Alpha() end\n"}, "worktree concurrent")
	var wg sync.WaitGroup
	results := make(chan error, 4)
	for range 4 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			lease, err := b.AcquireWorktree(context.Background(), pin)
			if err == nil {
				err = lease.Close()
			}
			results <- err
		}()
	}
	wg.Wait()
	close(results)
	for err := range results {
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestPruneSourceWorktreesKeepsActiveLeaseAndPins(t *testing.T) {
	b, seed := sourceFixture(t)
	one := testCommit(t, b, seed, map[string]string{"one.lua": "function One() end\n"}, "prune one")
	two := testCommit(t, b, seed, map[string]string{"two.lua": "function Two() end\n"}, "prune two")
	first, err := b.AcquireWorktree(context.Background(), one)
	if err != nil {
		t.Fatal(err)
	}
	second, err := b.AcquireWorktree(context.Background(), two)
	if err != nil {
		t.Fatal(err)
	}
	second.Close()
	result, err := PruneSourceWorktrees(context.Background(), b.store.Root(), 0)
	if err != nil {
		t.Fatal(err)
	}
	if result.Complete || len(result.Removed) != 1 {
		t.Fatalf("leased tree pruned or unbounded result: %+v", result)
	}
	if _, err := os.Stat(first.Path()); err != nil {
		t.Fatalf("active tree missing: %v", err)
	}
	if _, err := gitBytes(context.Background(), b.mirror(two.Repository), 128, "cat-file", "-e", two.ExactCommit+"^{commit}"); err != nil {
		t.Fatalf("pin object removed: %v", err)
	}
	first.Close()
	result, err = PruneSourceWorktrees(context.Background(), b.store.Root(), 0)
	if err != nil || !result.Complete {
		t.Fatalf("released tree not pruned: %+v %v", result, err)
	}
}
