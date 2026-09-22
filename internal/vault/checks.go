package vault

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// Check is the shared read-only health result shape. It lives in vault because
// vault is the common storage dependency of the use-case modules; each check
// ID and code stays module-qualified (vault.*, selection.*, ...). Data keeps
// the shape extensible for later doctor assembly (for example a codebase
// mirror check) without a god-module.
type Check struct {
	ID       string         `json:"id"`
	OK       bool           `json:"ok"`
	Status   string         `json:"status"` // ok | warn | error | unknown
	Code     string         `json:"code,omitempty"`
	Detail   string         `json:"detail"`
	NextStep string         `json:"nextStep,omitempty"`
	Data     map[string]any `json:"data,omitempty"`
}

func checkResult(id string, ok bool, status, code, detail, nextStep string) Check {
	return Check{ID: id, OK: ok, Status: status, Code: code, Detail: detail, NextStep: nextStep}
}

// WorkspaceChecks inspects one workspace root and the caller-supplied legacy
// candidates. Every check is independent and read-only except the writability
// probe, which creates and removes a probe file in tmp/ only.
func WorkspaceChecks(ctx context.Context, root string, legacy []LegacyRoot) []Check {
	checks := make([]Check, 0, 8)
	store, err := OpenStore(root)
	switch {
	case err == nil:
		checks = append(checks, checkResult("vault.workspace", true, "ok", "",
			"workspace marker valid: schema "+store.Identity().Schema+", id "+store.Identity().WorkspaceID, ""))
	case errors.Is(err, ErrWorkspaceSchemaNewer):
		checks = append(checks, checkResult("vault.workspace", false, "error", "vault.workspace_schema_newer",
			"workspace marker was written by a newer toolkit", "upgrade the toolkit; writes are refused to protect the data"))
	case errors.Is(err, ErrWorkspaceFormat):
		checks = append(checks, checkResult("vault.workspace", false, "error", "vault.unsupported_workspace",
			"workspace marker is corrupt or unsupported", "inspect workspace.json by hand or reinitialize into a fresh root; nothing is overwritten automatically"))
	case errors.Is(err, ErrLegacyWorkspace):
		checks = append(checks, checkResult("vault.workspace", false, "error", "vault.legacy_detected",
			"root is occupied without the new workspace marker", "run an explicit fresh init to isolate the legacy root, or pick another --home"))
	case errors.Is(err, os.ErrNotExist):
		checks = append(checks, checkResult("vault.workspace", false, "error", "vault.workspace_missing",
			"no workspace at "+root, "run init to create a workspace"))
	default:
		checks = append(checks, checkResult("vault.workspace", false, "error", "vault.unsupported_workspace",
			err.Error(), "check the workspace path and permissions"))
	}
	if store == nil {
		checks = append(checks, archiveCheck(root))
		return append(checks, legacyChecks(legacy)...)
	}
	checks = append(checks, metadataCheck(ctx, store)...)
	checks = append(checks, writableCheck(ctx, store))
	checks = append(checks, hygieneChecks(store)...)
	checks = append(checks, cacheCheck(ctx, store))
	checks = append(checks, archiveCheck(root))
	checks = append(checks, legacyChecks(legacy)...)
	return checks
}

func metadataCheck(ctx context.Context, store *Store) []Check {
	metadata, err := store.ReadMetadata(ctx)
	if errors.Is(err, ErrMissingRecord) {
		return []Check{checkResult("vault.metadata", false, "warn", "vault.record_missing",
			"metadata database not created yet", "run a command that records state, or ignore for a brand-new workspace")}
	}
	if err != nil {
		if errors.Is(err, ErrWorkspaceFormat) {
			return []Check{checkResult("vault.metadata", false, "error", "vault.workspace_schema_newer",
				"metadata schema is newer or corrupt: "+err.Error(), "upgrade the toolkit; unsafe writes are refused")}
		}
		return []Check{checkResult("vault.metadata", false, "error", "vault.unsupported_workspace",
			err.Error(), "inspect state/toolkit.sqlite")}
	}
	defer metadata.Close()
	var version int
	if err := metadata.db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return []Check{checkResult("vault.metadata", false, "error", "vault.unsupported_workspace",
			err.Error(), "inspect state/toolkit.sqlite")}
	}
	return []Check{checkResult("vault.metadata", true, "ok", "",
		"metadata readable; schema version "+strconv.Itoa(version), "")}
}

func writableCheck(ctx context.Context, store *Store) Check {
	probe := filepath.Join(store.Root(), "tmp", ".lycheedev-write-probe")
	f, err := os.OpenFile(probe, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return checkResult("vault.writable", false, "error", "vault.workspace_readonly",
			"cannot create a probe file in tmp/: "+err.Error(), "check filesystem permissions and re-run")
	}
	_, writeErr := f.Write([]byte("probe"))
	closeErr := f.Close()
	removeErr := os.Remove(probe)
	if err := errors.Join(writeErr, closeErr, removeErr); err != nil {
		return checkResult("vault.writable", false, "error", "vault.workspace_readonly",
			"write probe failed: "+err.Error(), "check filesystem permissions and re-run")
	}
	return checkResult("vault.writable", true, "ok", "", "workspace tmp/ accepts writes", "")
}

func hygieneChecks(store *Store) []Check {
	lockFiles := 0
	_ = walkFiles(filepath.Join(store.Root(), "locks"), func(path string, entry os.DirEntry) error {
		lockFiles++
		return nil
	})
	locks := checkResult("vault.lock_hygiene", true, "ok", "",
		fmt.Sprintf("%d lock files present; lock files are retained on purpose so lock identity never splits", lockFiles), "")
	stale, oldest := residueCount(filepath.Join(store.Root(), "tmp"))
	stale += countResidue(filepath.Join(store.Root(), "cache", "tmp"))
	if stale == 0 {
		return []Check{locks, checkResult("vault.tmp_hygiene", true, "ok", "", "no leftover temporary files", "")}
	}
	detail := fmt.Sprintf("%d leftover temporary file(s)", stale)
	if !oldest.IsZero() {
		detail += "; oldest modified " + oldest.UTC().Format("2006-01-02T15:04:05Z")
	}
	return []Check{locks, checkResult("vault.tmp_hygiene", false, "warn", "vault.stale_temporary_files",
		detail, "confirm no work is running, then remove stale temporary files or run cache prune --uncommitted")}
}

func cacheCheck(ctx context.Context, store *Store) Check {
	report, err := store.Cache().Verify(ctx)
	if err != nil {
		return checkResult("vault.cache_integrity", false, "error", "vault.cache_integrity",
			err.Error(), "run cache verify and inspect the managed cache")
	}
	detail := fmt.Sprintf("objects=%d bytes=%d missing=%d corrupt=%d", report.Checked, report.Bytes, len(report.Missing), len(report.Corrupt))
	if report.OK {
		return checkResult("vault.cache_integrity", true, "ok", "", detail, "")
	}
	return checkResult("vault.cache_integrity", false, "error", "vault.cache_integrity",
		detail, "run cache verify for the exact objects and cache prune to reclaim damaged ordinary cache")
}

func archiveCheck(root string) Check {
	plan, err := ArchiveJournal(root)
	if errors.Is(err, os.ErrNotExist) {
		return checkResult("vault.archive_state", true, "ok", "", "no archive switch recorded for this root", "")
	}
	if err != nil {
		return checkResult("vault.archive_state", false, "warn", "vault.archive_state",
			err.Error(), "inspect the archive-state journal next to the workspace root")
	}
	detail := fmt.Sprintf("archive switch state=%s archivePath=%s", plan.State, plan.ArchivePath)
	if plan.State == "complete" {
		return checkResult("vault.archive_state", true, "ok", "", detail, "")
	}
	return checkResult("vault.archive_state", false, "warn", "vault.archive_incomplete",
		detail, "resume the interrupted switch (init fresh completes it) before writing to this root")
}

func legacyChecks(roots []LegacyRoot) []Check {
	if len(roots) == 0 {
		return nil
	}
	inspected := InspectLegacyRoots(roots)
	present := make([]string, 0, len(inspected))
	data := make(map[string]any, len(inspected))
	for _, root := range inspected {
		if root.Detected {
			present = append(present, root.Kind+" at "+root.Path+" ("+root.Detail+")")
		}
		data[root.Kind] = map[string]any{"path": root.Path, "detected": root.Detected, "detail": root.Detail}
	}
	check := checkResult("vault.legacy_roots", true, "ok", "", "", "")
	check.Data = data
	if len(present) == 0 {
		check.Detail = "no legacy data locations detected"
		return []Check{check}
	}
	// Present-but-isolated is a healthy state: detection is existence and
	// marker sniffing only, and nothing is read, copied or deleted.
	check.Detail = strings.Join(present, "; ")
	check.NextStep = "legacy data stays in place and is never read; remove it manually whenever you like"
	return []Check{check}
}

func residueCount(root string) (int, time.Time) {
	count := countResidue(root)
	oldest := oldestModTime(root)
	return count, oldest
}

func countResidue(root string) int {
	count := 0
	_ = walkFiles(root, func(path string, entry os.DirEntry) error {
		count++
		return nil
	})
	return count
}

func oldestModTime(root string) time.Time {
	var oldest time.Time
	_ = walkFiles(root, func(path string, entry os.DirEntry) error {
		info, err := entry.Info()
		if err != nil {
			return nil
		}
		if oldest.IsZero() || info.ModTime().Before(oldest) {
			oldest = info.ModTime()
		}
		return nil
	})
	return oldest
}
