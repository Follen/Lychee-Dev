package vault

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func writeSentinel(t *testing.T, path, content string) string {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256([]byte(content))
	return hex.EncodeToString(sum[:])
}

func digestOf(t *testing.T, path string) string {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// legacyHomeFixture plants sentinel data in every legacy layout in scope for
// detection. Nothing in the new toolkit may read, copy or change it.
func legacyHomeFixture(t *testing.T) (string, string, map[string]string) {
	t.Helper()
	home := filepath.Join(t.TempDir(), "home")
	localAppData := filepath.Join(t.TempDir(), "local")
	sentinels := map[string]string{}
	record := func(path, content string) {
		sentinels[path] = writeSentinel(t, path, content)
	}
	for _, dir := range []string{filepath.Join(home, ".wowdoc", "catalog"), filepath.Join(home, ".wowdoc", "gitstore")} {
		if err := os.MkdirAll(dir, 0700); err != nil {
			t.Fatal(err)
		}
	}
	record(filepath.Join(home, ".wowdoc", "sentinel.txt"), "wowdoc user data\n")
	record(filepath.Join(home, ".wowdoc", "catalog", "facts.bin"), "\x00\x01wowdoc catalog\n")
	if err := os.MkdirAll(filepath.Join(home, ".wowdata", "profiles"), 0700); err != nil {
		t.Fatal(err)
	}
	record(filepath.Join(home, ".wowdata", "wowdata.config.v1"), `{"schema":"wowdata.config.v1","cacheMaxBytes":123456,"downloadWorkers":9}`+"\n")
	record(filepath.Join(home, ".wowdata", "sentinel.txt"), "wowdata user data\n")
	if err := os.MkdirAll(filepath.Join(home, ".lycheedev", "python"), 0700); err != nil {
		t.Fatal(err)
	}
	record(filepath.Join(home, ".lycheedev", "config.json"), `{"schema":"lycheedev.install.v1","legacy":true}`+"\n")
	record(filepath.Join(home, ".lycheedev", "sentinel.txt"), "old lycheedev home data\n")
	if err := os.MkdirAll(filepath.Join(localAppData, "LycheeDev", "automation"), 0700); err != nil {
		t.Fatal(err)
	}
	record(filepath.Join(localAppData, "LycheeDev", "automation", "automation.py"), "print('legacy')\n")
	record(filepath.Join(localAppData, "LycheeDev", "automation", "sentinel.txt"), "automation user data\n")
	return home, localAppData, sentinels
}

func assertDigests(t *testing.T, sentinels map[string]string) {
	t.Helper()
	for path, want := range sentinels {
		if got := digestOf(t, path); got != want {
			t.Fatalf("sentinel %s changed: %s != %s", path, got, want)
		}
	}
}

// STO-01: an empty or absent directory initializes into the new format only,
// and the workspace supports independent metadata, blob and pin use.
func TestFreshInitializeEmptyDirectoryCreatesNewFormat(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	for name, root := range map[string]string{
		"absent": filepath.Join(parent, "absent-home"),
		"empty":  mustMkdir(t, filepath.Join(parent, "empty-home")),
	} {
		t.Run(name, func(t *testing.T) {
			result, err := FreshInitialize(ctx, root)
			if err != nil {
				t.Fatalf("FreshInitialize() error = %v", err)
			}
			if result.Plan.State != "complete" || result.Plan.ArchivePath != "" {
				t.Fatalf("plan = %+v", result.Plan)
			}
			if result.Summary.Identity.Schema != WorkspaceSchema {
				t.Fatalf("identity = %+v", result.Summary.Identity)
			}
			for _, dir := range []string{"state", "pins", "mirrors", "blobs", "indexes", "cache", "runs", "captures", "locks", "tmp"} {
				if info, err := os.Stat(filepath.Join(root, dir)); err != nil || !info.IsDir() {
					t.Fatalf("workspace directory %q missing: %v", dir, err)
				}
			}
			_, err = WriteMetadata(ctx, root, func(store *Store, metadata *Metadata) (bool, error) {
				ref, err := store.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("payload"), MaxBytes: 1024})
				if err != nil {
					return false, err
				}
				return true, metadata.CommitDocuments(ctx, Mutation{Key: "probe/doc", Value: []byte(`{"sha256":"` + ref.SHA256 + `"}`)})
			})
			if err != nil {
				t.Fatalf("workspace use error = %v", err)
			}
		})
	}
}

func mustMkdir(t *testing.T, path string) string {
	t.Helper()
	if err := os.MkdirAll(path, 0700); err != nil {
		t.Fatal(err)
	}
	return path
}

// STO-02: legacy homes and the old LocalAppData automation directory hold
// sentinel data that is never read, copied or imported (digests unchanged,
// sentinel bytes appear nowhere in the new workspace) and legacy config values
// never leak into the new resource configuration.
func TestLegacySentinelsAreNeverReadOrCopied(t *testing.T) {
	ctx := context.Background()
	home, localAppData, sentinels := legacyHomeFixture(t)
	roots := InspectLegacyRoots(LegacyRootCandidates(home, localAppData))
	kinds := map[string]bool{}
	for _, root := range roots {
		if !root.Detected {
			t.Fatalf("legacy root not detected: %+v", root)
		}
		kinds[root.Kind] = true
	}
	for _, kind := range []string{"wowdoc", "wowdata", "lycheedev-legacy", "lycheedev-automation"} {
		if !kinds[kind] {
			t.Fatalf("legacy kind %s not detected: %v", kind, kinds)
		}
	}
	assertDigests(t, sentinels)

	workspace := filepath.Join(t.TempDir(), "fresh workspace")
	if _, err := Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	config, err := ReadConfig(ctx, workspace)
	if err != nil {
		t.Fatal(err)
	}
	if config != DefaultConfig() {
		t.Fatalf("config = %+v, want defaults (legacy wowdata.config.v1 must not be imported)", config)
	}
	// No sentinel content may appear anywhere in the new workspace.
	for path := range sentinels {
		content, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		err = filepath.WalkDir(workspace, func(walk string, entry os.DirEntry, walkErr error) error {
			if walkErr != nil || entry.IsDir() {
				return walkErr
			}
			raw, err := os.ReadFile(walk)
			if err != nil {
				return err
			}
			if strings.Contains(string(raw), strings.TrimSpace(string(content))) && strings.TrimSpace(string(content)) != "" {
				t.Errorf("sentinel %s content copied into workspace at %s", path, walk)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	assertDigests(t, sentinels)
}

// STO-03: an occupied root without the workspace marker returns
// vault.legacy_detected; only an explicit fresh initialization executes the
// resolved archive plan that isolates the legacy root.
func TestLegacyRootDetectedAndExplicitFreshArchives(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	root := filepath.Join(parent, ".lycheedev")
	sentinel := filepath.Join(root, "sentinel.txt")
	digest := writeSentinel(t, sentinel, "old root user data\n")
	if err := os.MkdirAll(filepath.Join(root, "python"), 0700); err != nil {
		t.Fatal(err)
	}
	writeSentinel(t, filepath.Join(root, "config.json"), `{"legacy":true}`+"\n")

	if _, err := OpenStore(root); !errors.Is(err, ErrLegacyWorkspace) {
		t.Fatalf("OpenStore() error = %v, want %v", err, ErrLegacyWorkspace)
	}
	if _, err := Initialize(ctx, root); !errors.Is(err, ErrLegacyWorkspace) {
		t.Fatalf("Initialize() error = %v, want %v", err, ErrLegacyWorkspace)
	}

	plan, err := PlanFreshInitialize(ctx, root)
	if err != nil {
		t.Fatalf("PlanFreshInitialize() error = %v", err)
	}
	if plan.State != "planned" || plan.ArchivePath == "" {
		t.Fatalf("plan = %+v", plan)
	}
	if filepath.Dir(plan.ArchivePath) != parent || !strings.HasPrefix(filepath.Base(plan.ArchivePath), ".lycheedev-legacy-") {
		t.Fatalf("archive path %q is not a deterministic sibling of %q", plan.ArchivePath, root)
	}
	if len(plan.Steps) != 3 || plan.Steps[0].ID != "journal" || plan.Steps[1].ID != "move" || plan.Steps[2].ID != "create" {
		t.Fatalf("steps = %+v", plan.Steps)
	}
	if len(plan.Markers) == 0 {
		t.Fatalf("legacy markers not recorded: %+v", plan)
	}

	result, err := FreshInitialize(ctx, root)
	if err != nil {
		t.Fatalf("FreshInitialize() error = %v", err)
	}
	if result.Plan.State != "complete" || result.Plan.ArchivePath != plan.ArchivePath {
		t.Fatalf("executed plan = %+v", result.Plan)
	}
	if _, err := OpenStore(root); err != nil {
		t.Fatalf("new workspace not created: %v", err)
	}
	if got := digestOf(t, filepath.Join(plan.ArchivePath, "sentinel.txt")); got != digest {
		t.Fatalf("archived sentinel changed: %s != %s", got, digest)
	}
	journal, err := ArchiveJournal(root)
	if err != nil || journal.State != "complete" {
		t.Fatalf("journal = %+v, %v", journal, err)
	}
}

// STO-04: interruption at every archive step leaves an explainable state that
// the next call completes. No half-new/half-old root and no wrong-root writes.
func TestArchiveInterruptAndResumeEveryStep(t *testing.T) {
	ctx := context.Background()
	steps := []string{"journal", "move", "create"}
	for _, stopAfter := range steps {
		t.Run(stopAfter, func(t *testing.T) {
			parent := t.TempDir()
			root := filepath.Join(parent, "lychee dev root")
			original := map[string]string{}
			original["sentinel.txt"] = writeSentinel(t, filepath.Join(root, "sentinel.txt"), "user data\n")
			original["extra user file.txt"] = writeSentinel(t, filepath.Join(root, "extra user file.txt"), "extra\n")
			if err := os.MkdirAll(filepath.Join(root, "python"), 0700); err != nil {
				t.Fatal(err)
			}

			plan, err := PlanFreshInitialize(ctx, root)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := runArchive(ctx, plan, stopAfter); !errors.Is(err, errArchiveInterrupt) {
				t.Fatalf("runArchive(%s) error = %v, want interruption", stopAfter, err)
			}

			journal, err := ArchiveJournal(root)
			if err != nil {
				t.Fatalf("journal missing after interruption: %v", err)
			}
			switch stopAfter {
			case "journal":
				if journal.State != "planned" {
					t.Fatalf("journal state = %q", journal.State)
				}
				if _, err := os.Lstat(root); err != nil {
					t.Fatalf("legacy root moved too early: %v", err)
				}
				if _, err := os.Stat(filepath.Join(root, "workspace.json")); !errors.Is(err, os.ErrNotExist) {
					t.Fatal("new marker written into the legacy root")
				}
				if _, err := os.Lstat(plan.ArchivePath); !errors.Is(err, os.ErrNotExist) {
					t.Fatal("archive created before the move step")
				}
			case "move":
				if journal.State != "moved" {
					t.Fatalf("journal state = %q", journal.State)
				}
				if _, err := os.Lstat(root); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("legacy root still present after move: %v", err)
				}
				if _, err := os.Lstat(plan.ArchivePath); err != nil {
					t.Fatalf("archive missing after move: %v", err)
				}
			case "create":
				if _, err := OpenStore(root); err != nil {
					t.Fatalf("new workspace missing after create: %v", err)
				}
				if journal.State != "moved" {
					t.Fatalf("journal state = %q, want moved before completion record", journal.State)
				}
			}

			// Resume completes from the recorded state alone.
			result, err := CompleteArchive(ctx, root)
			if err != nil {
				t.Fatalf("CompleteArchive() error = %v", err)
			}
			if result.Plan.State != "complete" || result.Plan.ArchivePath != plan.ArchivePath {
				t.Fatalf("resumed plan = %+v", result.Plan)
			}
			if _, err := OpenStore(root); err != nil {
				t.Fatalf("final workspace invalid: %v", err)
			}
			// The legacy content moved wholesale: same files, same bytes, and
			// nothing was written into the archive or into the wrong root.
			for name, want := range original {
				if got := digestOf(t, filepath.Join(plan.ArchivePath, name)); got != want {
					t.Fatalf("archived %s changed: %s != %s", name, got, want)
				}
				if _, err := os.Lstat(filepath.Join(root, name)); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("legacy file %s also appeared in the new root", name)
				}
			}
			assertTreeNames(t, plan.ArchivePath, []string{"extra user file.txt", "python", "sentinel.txt"})
		})
	}
}

func assertTreeNames(t *testing.T, root string, want []string) {
	t.Helper()
	got := make([]string, 0)
	if err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil || path == root {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		got = append(got, filepath.ToSlash(rel))
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	joined := strings.Join(got, "|")
	for _, name := range want {
		if !strings.Contains(joined, name) {
			t.Fatalf("tree %v missing %s", got, name)
		}
	}
	for _, name := range got {
		if strings.Contains(name, "workspace.json") || strings.Contains(name, "archive-state") {
			t.Fatalf("wrong-root write inside archive: %s", name)
		}
	}
}

// STO-05: a newer or corrupt workspace/schema refuses unsafe writes and is
// never overwritten as if it were an empty configuration.
func TestNewerOrCorruptSchemaRefusesUnsafeWrites(t *testing.T) {
	ctx := context.Background()
	validID := strings.Repeat("a", 32)

	t.Run("newer workspace marker", func(t *testing.T) {
		parent := t.TempDir()
		root := filepath.Join(parent, "root")
		writeSentinel(t, filepath.Join(root, "workspace.json"),
			`{"schema":"lycheedev.workspace.v2","workspaceId":"`+validID+`","createdAt":"2026-09-21T00:00:00Z"}`)
		digest := digestOf(t, filepath.Join(root, "workspace.json"))
		_, err := OpenStore(root)
		if !errors.Is(err, ErrWorkspaceSchemaNewer) || !errors.Is(err, ErrWorkspaceFormat) {
			t.Fatalf("OpenStore() error = %v, want newer + format", err)
		}
		if _, err := PlanFreshInitialize(ctx, root); !errors.Is(err, ErrWorkspaceSchemaNewer) {
			t.Fatalf("PlanFreshInitialize() error = %v, want %v", err, ErrWorkspaceSchemaNewer)
		}
		if _, err := FreshInitialize(ctx, root); !errors.Is(err, ErrWorkspaceSchemaNewer) {
			t.Fatalf("FreshInitialize() error = %v, want %v", err, ErrWorkspaceSchemaNewer)
		}
		if digestOf(t, filepath.Join(root, "workspace.json")) != digest {
			t.Fatal("refused initialization modified the newer marker")
		}
		entries, err := os.ReadDir(parent)
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range entries {
			if strings.Contains(entry.Name(), "legacy") || strings.Contains(entry.Name(), "archive-state") {
				t.Fatalf("refused initialization created %s", entry.Name())
			}
		}
	})

	t.Run("corrupt workspace marker", func(t *testing.T) {
		root := filepath.Join(t.TempDir(), "root")
		writeSentinel(t, filepath.Join(root, "workspace.json"), "{")
		if _, err := OpenStore(root); !errors.Is(err, ErrWorkspaceFormat) {
			t.Fatalf("OpenStore() error = %v, want %v", err, ErrWorkspaceFormat)
		}
		if _, err := PlanFreshInitialize(ctx, root); !errors.Is(err, ErrWorkspaceFormat) {
			t.Fatalf("PlanFreshInitialize() error = %v, want %v", err, ErrWorkspaceFormat)
		}
	})

	t.Run("newer and corrupt config refuse writes", func(t *testing.T) {
		root := filepath.Join(t.TempDir(), "root")
		if _, err := Initialize(ctx, root); err != nil {
			t.Fatal(err)
		}
		newer := filepath.Join(root, "config.json")
		writeSentinel(t, newer, `{"schema":"lycheedev.config.v2","cache":{"maxBytes":1},"download":{"workers":1}}`+"\n")
		before := digestOf(t, newer)
		if _, err := ReadConfig(ctx, root); !errors.Is(err, ErrConfigSchemaNewer) {
			t.Fatalf("ReadConfig() error = %v, want %v", err, ErrConfigSchemaNewer)
		}
		if _, err := WriteConfig(ctx, root, DefaultConfig()); !errors.Is(err, ErrConfigSchemaNewer) {
			t.Fatalf("WriteConfig() error = %v, want %v", err, ErrConfigSchemaNewer)
		}
		if _, err := UpdateConfig(ctx, root, func(*Config) error { return nil }); !errors.Is(err, ErrConfigSchemaNewer) {
			t.Fatalf("UpdateConfig() error = %v, want %v", err, ErrConfigSchemaNewer)
		}
		if digestOf(t, newer) != before {
			t.Fatal("refused writes overwrote the newer configuration")
		}

		writeSentinel(t, newer, "{")
		before = digestOf(t, newer)
		if _, err := ReadConfig(ctx, root); !errors.Is(err, ErrConfigFormat) {
			t.Fatalf("ReadConfig() error = %v, want %v", err, ErrConfigFormat)
		}
		if _, err := WriteConfig(ctx, root, DefaultConfig()); !errors.Is(err, ErrConfigFormat) {
			t.Fatalf("WriteConfig() error = %v, want %v", err, ErrConfigFormat)
		}
		if digestOf(t, newer) != before {
			t.Fatal("refused writes overwrote the corrupt configuration")
		}
	})

	t.Run("newer metadata schema refuses writes", func(t *testing.T) {
		root := filepath.Join(t.TempDir(), "root")
		store, err := Initialize(ctx, root)
		if err != nil {
			t.Fatal(err)
		}
		metadata, err := store.OpenMetadata(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := metadata.db.Exec("PRAGMA user_version=2"); err != nil {
			t.Fatal(err)
		}
		if err := metadata.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := store.OpenMetadata(ctx); !errors.Is(err, ErrWorkspaceFormat) {
			t.Fatalf("OpenMetadata() error = %v, want %v", err, ErrWorkspaceFormat)
		}
		if _, err := store.ReadMetadata(ctx); !errors.Is(err, ErrWorkspaceFormat) {
			t.Fatalf("ReadMetadata() error = %v, want %v", err, ErrWorkspaceFormat)
		}
	})
}

// STO-06: two complete homes with Unicode and space paths stay isolated, and
// the workspace root is a complete root path (no parent-joining semantics).
func TestTwoHomesWithUnicodeAndSpacePathsAreIsolated(t *testing.T) {
	ctx := context.Background()
	base := t.TempDir()
	rootA := filepath.Join(base, "\u7528\u6237 A", "Lychee Dev \u5de5\u4f5c\u533a")
	rootB := filepath.Join(base, "\u03a9 user", "\u5de5\u4f5c\u533a two")
	for _, root := range []string{rootA, rootB} {
		if err := os.MkdirAll(root, 0700); err != nil {
			t.Fatal(err)
		}
		if _, err := Initialize(ctx, root); err != nil {
			t.Fatalf("Initialize(%s) error = %v", root, err)
		}
	}
	storeA, err := OpenStore(rootA)
	if err != nil {
		t.Fatal(err)
	}
	storeB, err := OpenStore(rootB)
	if err != nil {
		t.Fatal(err)
	}
	if storeA.Identity().WorkspaceID == storeB.Identity().WorkspaceID {
		t.Fatal("two homes share a workspace identity")
	}
	if _, err := WriteConfig(ctx, rootA, Config{Schema: ConfigSchema, Cache: CacheBudget{MaxBytes: 4096}, Download: DownloadBudget{Workers: 2}}); err != nil {
		t.Fatal(err)
	}
	configA, err := ReadConfig(ctx, rootA)
	if err != nil || configA.Cache.MaxBytes != 4096 || configA.Download.Workers != 2 {
		t.Fatalf("config A = %+v, %v", configA, err)
	}
	configB, err := ReadConfig(ctx, rootB)
	if err != nil || configB != DefaultConfig() {
		t.Fatalf("config B = %+v, %v; want untouched defaults", configB, err)
	}

	// A legacy root archived next to home A never touches home B.
	parentA := filepath.Join(base, "\u7528\u6237 A")
	legacy := filepath.Join(parentA, "old \u6570\u636e")
	digest := writeSentinel(t, filepath.Join(legacy, "sentinel.txt"), "legacy in home A\n")
	bSnapshot := treeSnapshot(t, rootB)
	result, err := FreshInitialize(ctx, legacy)
	if err != nil {
		t.Fatal(err)
	}
	if got := digestOf(t, filepath.Join(result.Plan.ArchivePath, "sentinel.txt")); got != digest {
		t.Fatal("archived sentinel changed")
	}
	if after := treeSnapshot(t, rootB); strings.Join(after, "|") != strings.Join(bSnapshot, "|") {
		t.Fatalf("home B changed: %v -> %v", bSnapshot, after)
	}
}

func treeSnapshot(t *testing.T, root string) []string {
	t.Helper()
	names := make([]string, 0)
	if err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		names = append(names, filepath.ToSlash(rel))
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	return names
}
