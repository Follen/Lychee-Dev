package vault

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"
)

// ArchiveSchema versions the archive journal that makes the legacy-root switch
// interruptible and resumable.
const ArchiveSchema = "lycheedev.archive.v1"

var (
	// ErrWorkspaceExists refuses destructive "fresh" initialization of a root
	// that already holds a valid new-format workspace.
	ErrWorkspaceExists = errors.New("vault.workspace_exists")
	// ErrArchiveState reports a mid-archive state that cannot be completed or
	// explained safely. It never guesses and never deletes anything.
	ErrArchiveState = errors.New("vault.archive_state")
	// errArchiveInterrupt is the internal test seam used to stop after one
	// recorded archive step; public entry points never return it.
	errArchiveInterrupt = errors.New("vault.archive_interrupt")
)

// ArchiveStep is one idempotent, separately recorded archive action.
type ArchiveStep struct {
	ID     string `json:"id"`
	Detail string `json:"detail"`
}

// ArchivePlan is the resolved isolation plan for one root. ArchivePath names
// the deterministic sibling archive directory; JournalPath is the durable
// record that lets an interrupted switch resume precisely.
type ArchivePlan struct {
	Schema      string        `json:"schema"`
	Root        string        `json:"root"`
	ArchivePath string        `json:"archivePath,omitempty"`
	JournalPath string        `json:"journalPath"`
	Markers     []string      `json:"markers,omitempty"`
	Steps       []ArchiveStep `json:"steps"`
	State       string        `json:"state"`
	CreatedAt   time.Time     `json:"createdAt"`
	CompletedAt *time.Time    `json:"completedAt,omitempty"`
}

// FreshResult reports the executed (or resumed) plan together with the new
// workspace. The legacy root is isolated, never merged and never deleted.
type FreshResult struct {
	Plan    ArchivePlan      `json:"plan"`
	Summary WorkspaceSummary `json:"summary"`
}

// PlanFreshInitialize resolves the archive plan for an explicit fresh
// initialization without changing anything on disk. An occupied root without
// the new workspace marker is planned for isolation into
// "<parent>/<base>-legacy-<UTC timestamp>"; a root that already carries a
// workspace marker is refused instead of archived. An existing journal is
// adopted verbatim so a resumed switch keeps its original archive path.
func PlanFreshInitialize(ctx context.Context, root string) (ArchivePlan, error) {
	var plan ArchivePlan
	abs, err := filepath.Abs(root)
	if err != nil {
		return plan, err
	}
	if err := ctx.Err(); err != nil {
		return plan, err
	}
	parent := filepath.Dir(abs)
	if abs == parent {
		return plan, fmt.Errorf("vault: workspace cannot be a filesystem root")
	}
	journalPath := filepath.Join(parent, filepath.Base(abs)+".archive-state.json")
	if journal, readErr := readArchiveJournal(journalPath, abs); readErr == nil {
		return journal, nil
	} else if !errors.Is(readErr, os.ErrNotExist) {
		return plan, readErr
	}
	state, markers, err := inspectRootState(abs)
	if err != nil {
		return plan, err
	}
	now := time.Now().UTC()
	switch state {
	case "absent", "empty":
		plan = ArchivePlan{
			Schema: ArchiveSchema, Root: abs, JournalPath: journalPath, State: "planned", CreatedAt: now,
			Steps: []ArchiveStep{{ID: "create", Detail: "create new-format workspace at " + abs}},
		}
	case "workspace":
		return plan, ErrWorkspaceExists
	case "legacy":
		archivePath, err := resolveArchivePath(parent, filepath.Base(abs), now)
		if err != nil {
			return plan, err
		}
		plan = ArchivePlan{
			Schema: ArchiveSchema, Root: abs, ArchivePath: archivePath, JournalPath: journalPath,
			Markers: markers, State: "planned", CreatedAt: now,
			Steps: []ArchiveStep{
				{ID: "journal", Detail: "record the resolved archive plan at " + journalPath},
				{ID: "move", Detail: "isolate the legacy root into " + archivePath},
				{ID: "create", Detail: "create new-format workspace at " + abs},
			},
		}
	default:
		return plan, fmt.Errorf("%w: unclassified root state %q", ErrArchiveState, state)
	}
	return plan, nil
}

// FreshInitialize performs an EXPLICIT fresh initialization. It first resolves
// and records the archive plan, then isolates an occupied legacy root into the
// deterministic sibling archive directory and only then creates the new root.
// It never merges, never auto-deletes and never imports legacy data. The
// switch is interruptible and resumable: every step is idempotent and recorded,
// and a later call completes or precisely reports a crash-mid-archive state.
func FreshInitialize(ctx context.Context, root string) (FreshResult, error) {
	plan, err := PlanFreshInitialize(ctx, root)
	if err != nil {
		return FreshResult{}, err
	}
	return runArchive(ctx, plan, "")
}

// CompleteArchive resumes an interrupted archive switch from its journal. It
// reports vault.archive_state when there is nothing to resume.
func CompleteArchive(ctx context.Context, root string) (FreshResult, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return FreshResult{}, err
	}
	journalPath := filepath.Join(filepath.Dir(abs), filepath.Base(abs)+".archive-state.json")
	plan, err := readArchiveJournal(journalPath, abs)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return FreshResult{}, fmt.Errorf("%w: no archive journal for %s", ErrArchiveState, abs)
		}
		return FreshResult{}, err
	}
	return runArchive(ctx, plan, "")
}

func runArchive(ctx context.Context, plan ArchivePlan, stopAfter string) (FreshResult, error) {
	var result FreshResult
	abs := plan.Root
	parent := filepath.Dir(abs)
	lease, err := AcquireLease(ctx, filepath.Join(parent, ".lycheedev-locks"), "initialize:"+filepath.Base(abs))
	if err != nil {
		return result, err
	}
	defer lease.Close()

	if plan.ArchivePath == "" {
		// No legacy content to isolate: creating the new root is the whole plan.
		store, err := initializeLocked(ctx, abs)
		if err != nil {
			return result, err
		}
		plan.State = "complete"
		now := time.Now().UTC()
		plan.CompletedAt = &now
		return FreshResult{Plan: plan, Summary: WorkspaceSummary{Root: store.Root(), Identity: store.Identity()}}, nil
	}

	// Step "journal": persist the resolved plan before anything moves.
	if _, err := os.Lstat(plan.JournalPath); err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			return result, err
		}
		if err := writeArchiveJournal(plan); err != nil {
			return result, err
		}
	} else if journal, err := readArchiveJournal(plan.JournalPath, abs); err != nil {
		return result, err
	} else {
		plan.State, plan.CompletedAt = journal.State, journal.CompletedAt
	}
	if stopAfter == "journal" {
		return result, errArchiveInterrupt
	}
	if plan.State == "complete" {
		// A finished switch is idempotent: reopen the workspace it created.
		store, err := OpenStore(abs)
		if err != nil {
			return result, err
		}
		return FreshResult{Plan: plan, Summary: WorkspaceSummary{Root: store.Root(), Identity: store.Identity()}}, nil
	}

	// Step "move": rename the whole legacy root into the sibling archive.
	if plan.State == "planned" {
		switch state, _, err := inspectRootState(abs); {
		case err != nil:
			return result, err
		case state == "workspace":
			return result, fmt.Errorf("%w: root became a workspace mid-archive", ErrArchiveState)
		case state == "legacy" || state == "empty":
			if err := os.Rename(abs, plan.ArchivePath); err != nil {
				return result, err
			}
		case state == "absent":
			if archiveState, _, err := inspectRootState(plan.ArchivePath); err != nil {
				return result, err
			} else if archiveState == "absent" {
				return result, fmt.Errorf("%w: root and archive path are both missing", ErrArchiveState)
			}
		}
		plan.State = "moved"
		if err := writeArchiveJournal(plan); err != nil {
			return result, err
		}
	}
	if stopAfter == "move" {
		return result, errArchiveInterrupt
	}

	// Step "create": publish the new workspace, then mark the switch complete.
	store, err := initializeLocked(ctx, abs)
	if err != nil {
		return result, err
	}
	if stopAfter == "create" {
		return result, errArchiveInterrupt
	}
	plan.State = "complete"
	now := time.Now().UTC()
	plan.CompletedAt = &now
	if err := writeArchiveJournal(plan); err != nil {
		return result, err
	}
	return FreshResult{Plan: plan, Summary: WorkspaceSummary{Root: store.Root(), Identity: store.Identity()}}, nil
}

// ArchiveJournal returns the recorded archive state for a root, or an error
// wrapping os.ErrNotExist when no switch was ever planned. Health reporting
// uses it to surface unfinished switches without touching legacy data.
func ArchiveJournal(root string) (ArchivePlan, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return ArchivePlan{}, err
	}
	return readArchiveJournal(filepath.Join(filepath.Dir(abs), filepath.Base(abs)+".archive-state.json"), abs)
}

func writeArchiveJournal(plan ArchivePlan) error {
	data, err := json.MarshalIndent(plan, "", "  ")
	if err != nil {
		return err
	}
	return writeDurableReplace(filepath.Dir(plan.JournalPath), filepath.Base(plan.JournalPath), append(data, '\n'))
}

func readArchiveJournal(path, root string) (ArchivePlan, error) {
	var plan ArchivePlan
	info, err := os.Lstat(path)
	if err != nil {
		return plan, err
	}
	if !info.Mode().IsRegular() || info.Size() > 65536 {
		return plan, fmt.Errorf("%w: archive journal shape", ErrArchiveState)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return plan, err
	}
	if len(raw) > 65536 || !utf8.Valid(raw) {
		return plan, fmt.Errorf("%w: archive journal encoding", ErrArchiveState)
	}
	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&plan); err != nil {
		return plan, fmt.Errorf("%w: archive journal format: %v", ErrArchiveState, err)
	}
	if plan.Schema != ArchiveSchema || plan.Root != root || plan.JournalPath != path {
		return plan, fmt.Errorf("%w: archive journal identity", ErrArchiveState)
	}
	switch plan.State {
	case "planned", "moved", "complete":
	default:
		return plan, fmt.Errorf("%w: archive journal state %q", ErrArchiveState, plan.State)
	}
	return plan, nil
}

// inspectRootState classifies a root by existence and marker presence only.
// The only file it opens is the new-format workspace marker itself.
func inspectRootState(abs string) (string, []string, error) {
	info, err := os.Lstat(abs)
	if errors.Is(err, os.ErrNotExist) {
		return "absent", nil, nil
	}
	if err != nil {
		return "", nil, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", nil, ErrWorkspaceFormat
	}
	entries, err := os.ReadDir(abs)
	if err != nil {
		return "", nil, err
	}
	if len(entries) == 0 {
		return "empty", nil, nil
	}
	if _, err := os.Lstat(filepath.Join(abs, "workspace.json")); err == nil {
		if _, err := OpenStore(abs); err != nil {
			return "", nil, err
		}
		return "workspace", nil, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", nil, err
	}
	return "legacy", sniffMarkers(abs, legacyRootMarkers), nil
}

// legacyRootMarkers are marker NAMES for format sniffing of an occupied root
// without the workspace marker. Existence is observed; contents are not read.
var legacyRootMarkers = []string{
	"automation.py", "bin", "builds", "cache", "catalog", "config.json", "dist",
	"gitstore", "indexer", "instances.py", "npm", "objectstore", "output",
	"profiles", "python", "skill", "state", "storage", "store", "task-registry.json",
	"wowdata.config.v1",
}

// resolveArchivePath picks the deterministic sibling archive directory
// "<base>-legacy-<UTC timestamp>" and resolves collisions with a numeric
// suffix. The chosen path is recorded in the journal and reused on resume.
func resolveArchivePath(parent, base string, now time.Time) (string, error) {
	stamp := now.Format("20060102T150405Z")
	for attempt := 1; attempt <= 100; attempt++ {
		name := base + "-legacy-" + stamp
		if attempt > 1 {
			name = fmt.Sprintf("%s-legacy-%s-%d", base, stamp, attempt)
		}
		candidate := filepath.Join(parent, name)
		if _, err := os.Lstat(candidate); errors.Is(err, os.ErrNotExist) {
			return candidate, nil
		} else if err != nil {
			return "", err
		}
	}
	return "", fmt.Errorf("%w: no free archive path next to %s", ErrArchiveState, parent)
}

// writeDurableReplace publishes small state files atomically: existing readers
// keep the old file and later readers see the complete new file.
func writeDurableReplace(dir, name string, data []byte) error {
	tmp, err := os.CreateTemp(dir, ".lycheedev-state-")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	_, writeErr := tmp.Write(data)
	if writeErr == nil {
		writeErr = tmp.Sync()
	}
	if err := errors.Join(writeErr, tmp.Close()); err != nil {
		return err
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return err
	}
	defer root.Close()
	return root.Rename(filepath.Base(tmp.Name()), name)
}
