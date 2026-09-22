package vault

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func findCheck(checks []Check, id string) (Check, bool) {
	for _, check := range checks {
		if check.ID == id {
			return check, true
		}
	}
	return Check{}, false
}

func TestWorkspaceChecksCoverHygieneCacheAndLegacy(t *testing.T) {
	ctx := context.Background()
	home, localAppData, sentinels := legacyHomeFixture(t)
	root := filepath.Join(t.TempDir(), "workspace")
	store, err := Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := WriteMetadata(ctx, root, func(s *Store, m *Metadata) (bool, error) {
		_, err := s.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("check"), MaxBytes: 16})
		return err == nil, err
	}); err != nil {
		t.Fatal(err)
	}

	checks := WorkspaceChecks(ctx, root, LegacyRootCandidates(home, localAppData))
	for _, id := range []string{"vault.workspace", "vault.metadata", "vault.writable", "vault.lock_hygiene", "vault.tmp_hygiene", "vault.cache_integrity", "vault.archive_state", "vault.legacy_roots"} {
		if _, ok := findCheck(checks, id); !ok {
			t.Fatalf("missing check %s in %+v", id, checks)
		}
	}
	for _, check := range checks {
		if !check.OK {
			t.Fatalf("check failed on a healthy workspace: %+v", check)
		}
	}
	legacy, _ := findCheck(checks, "vault.legacy_roots")
	if legacy.Data["wowdata"] == nil {
		t.Fatalf("legacy detection data missing: %+v", legacy.Data)
	}
	assertDigests(t, sentinels)

	// Leftover temporary files surface as an actionable warning, never an
	// automatic deletion.
	if err := os.WriteFile(filepath.Join(store.Root(), "tmp", "leftover.part"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	checks = WorkspaceChecks(ctx, root, nil)
	tmp, _ := findCheck(checks, "vault.tmp_hygiene")
	if tmp.OK || tmp.Status != "warn" || tmp.NextStep == "" {
		t.Fatalf("tmp hygiene = %+v", tmp)
	}
}

func TestWorkspaceChecksReportMissingAndLegacyRoots(t *testing.T) {
	ctx := context.Background()
	missing := filepath.Join(t.TempDir(), "nothing-here")
	checks := WorkspaceChecks(ctx, missing, nil)
	workspace, _ := findCheck(checks, "vault.workspace")
	if workspace.OK || workspace.Code != "vault.workspace_missing" {
		t.Fatalf("workspace check = %+v", workspace)
	}

	parent := t.TempDir()
	root := filepath.Join(parent, "old")
	writeSentinel(t, filepath.Join(root, "config.json"), "{}\n")
	checks = WorkspaceChecks(ctx, root, nil)
	workspace, _ = findCheck(checks, "vault.workspace")
	if workspace.OK || workspace.Code != "vault.legacy_detected" {
		t.Fatalf("workspace check = %+v", workspace)
	}

	// An interrupted archive switch is reported precisely with a next step.
	plan, err := PlanFreshInitialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := runArchive(ctx, plan, "move"); err == nil {
		t.Fatal("expected interruption")
	}
	checks = WorkspaceChecks(ctx, root, nil)
	archive, _ := findCheck(checks, "vault.archive_state")
	if archive.OK || archive.Code != "vault.archive_incomplete" || archive.NextStep == "" {
		t.Fatalf("archive check = %+v", archive)
	}
}
