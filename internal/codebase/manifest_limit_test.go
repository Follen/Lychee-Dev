package codebase

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func padManifest(t *testing.T, path string) []byte {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(raw) > 64<<10 {
		t.Fatal("fixture manifest exceeds bound")
	}
	return append(raw, []byte(strings.Repeat(" ", (64<<10)-len(raw)))...)
}

func TestIndexRejectsManifestBytesBeyondLimit(t *testing.T) {
	ctx := context.Background()
	b, pin := sourceFixture(t)
	if _, err := b.IndexSource(ctx, pin); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(b.indexPath(pin), "manifest.json")
	bounded := padManifest(t, file)
	if err := os.WriteFile(file, bounded, 0600); err != nil {
		t.Fatal(err)
	}
	query := SearchQuery{Text: "missing", Limit: 10}
	if _, err := OpenBrowser(b.store).Search(ctx, "PIN-TEST", pin, query); err != nil {
		t.Fatal("exact limit rejected", err)
	}
	if err := os.WriteFile(file, append(bounded, []byte("garbage")...), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenBrowser(b.store).Search(ctx, "PIN-TEST", pin, query); !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("oversized manifest accepted: %v", err)
	}
}

func TestPruneRejectsManifestBytesBeyondLimitBeforeRemoval(t *testing.T) {
	ctx := context.Background()
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, map[string]string{"main.lua": "function Example() end\n"}, "manifest bound")
	checkout, err := b.AcquireWorktree(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	checkout.Close()
	file := b.worktreeManifestPath(pin)
	bounded := padManifest(t, file)
	if err = os.WriteFile(file, bounded, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = PruneSourceWorktrees(ctx, b.store.Root(), 1<<40); err != nil {
		t.Fatal("exact limit rejected", err)
	}
	if err = os.WriteFile(file, append(bounded, []byte("garbage")...), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = PruneSourceWorktrees(ctx, b.store.Root(), 0); !errors.Is(err, ErrSourceIntegrity) {
		t.Fatalf("oversized manifest authorized removal: %v", err)
	}
	if _, err = os.Stat(checkout.Path()); err != nil {
		t.Fatal("checkout changed before rejecting invalid manifest", err)
	}
}
