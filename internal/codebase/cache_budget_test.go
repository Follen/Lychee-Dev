package codebase

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func budgetBrowser(t *testing.T) (*Browser, string) {
	t.Helper()
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	return OpenBrowser(store), filepath.Join(store.Root(), "source", "v1")
}

func budgetFile(t *testing.T, path string, size int) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(strings.Repeat("x", size)), 0600); err != nil {
		t.Fatal(err)
	}
}

func TestSourceCapacityCountsPreservedFactsAndReclaimsOnlyDerived(t *testing.T) {
	b, root := budgetBrowser(t)
	key := strings.Repeat("a", 64)
	facts := filepath.Join(root, "facts", "files", "source-v1", "fixed.json")
	env := filepath.Join(root, "environments", key)
	semantic := filepath.Join(root, "semantics", strings.Repeat("b", 64)+".json")
	budgetFile(t, facts, 60)
	budgetFile(t, filepath.Join(env, "api.d.lua"), 40)
	budgetFile(t, filepath.Join(env, "manifest.json"), 10)
	budgetFile(t, semantic, 30)
	lease, err := b.reserveSourceCapacityBudget(context.Background(), 50, "", 130)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	if _, err := os.Stat(facts); err != nil {
		t.Fatalf("preserved facts removed: %v", err)
	}
	if _, err := os.Stat(env); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("idle environment not reclaimed: %v", err)
	}
	if _, err := os.Stat(semantic); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("idle semantic cache not reclaimed: %v", err)
	}
}

func TestSourceCapacityProtectsActiveKeyAndUnknownContent(t *testing.T) {
	b, root := budgetBrowser(t)
	key := strings.Repeat("a", 64)
	env := filepath.Join(root, "environments", key)
	budgetFile(t, filepath.Join(env, "api.d.lua"), 80)
	active, err := vault.AcquireLease(context.Background(), filepath.Join(b.store.Root(), "locks"), "source:v1:environment:"+key)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := b.reserveSourceCapacityBudget(context.Background(), 30, "", 100); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("active cache evicted or untyped quota: %v", err)
	}
	active.Close()
	if _, err := b.reserveSourceCapacityBudget(context.Background(), 30, env, 100); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("protected target evicted: %v", err)
	}
	budgetFile(t, filepath.Join(env, "user-extra.txt"), 1)
	if _, err := b.reserveSourceCapacityBudget(context.Background(), 30, "", 100); !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("unknown content removed or misclassified: %v", err)
	}
	if _, err := os.Stat(filepath.Join(env, "user-extra.txt")); err != nil {
		t.Fatal(err)
	}
}

func TestSourceCapacityGrowKeepsUnwrittenReservation(t *testing.T) {
	b, root := budgetBrowser(t)
	facts := filepath.Join(root, "facts", "files", "fixed.json")
	stage := filepath.Join(root, "facts", "snapshots", ".stage")
	budgetFile(t, facts, 20)
	lease, err := b.reserveSourceCapacityBudget(context.Background(), 40, "", 100)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	budgetFile(t, stage, 30)
	if err := lease.Grow(context.Background(), 40); err != nil {
		t.Fatal(err)
	}
	budgetFile(t, stage, 70)
	if err := lease.Grow(context.Background(), 20); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("grow overcommitted: %v", err)
	}
	if _, err := os.Stat(facts); err != nil {
		t.Fatalf("facts removed by quota: %v", err)
	}
}

func TestSourceCapacityCountsWorktreeBytes(t *testing.T) {
	b, root := budgetBrowser(t)
	budgetFile(t, filepath.Join(root, "worktrees", "wow-ui-source", strings.Repeat("a", 40), "generated.lua"), 90)
	if _, err := b.reserveSourceCapacityBudget(context.Background(), 20, "", 100); !errors.Is(err, ErrSourceBudget) {
		t.Fatalf("worktree omitted from total: %v", err)
	}
}

func TestSourceCapacityRejectsLinks(t *testing.T) {
	b, root := budgetBrowser(t)
	if err := os.MkdirAll(filepath.Join(root, "facts"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(root, "outside"), filepath.Join(root, "facts", "linked")); err != nil {
		t.Skipf("symlink unavailable on this host: %v", err)
	}
	if _, err := b.reserveSourceCapacityBudget(context.Background(), 0, "", 100); !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("source link accepted: %v", err)
	}
}
