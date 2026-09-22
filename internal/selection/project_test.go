package selection

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestInitializeProjectIsUnlockedIdempotentAndProtectsForeignFiles(t *testing.T) {
	ctx := context.Background()
	directory := t.TempDir()

	first, err := InitializeProject(ctx, directory, "retail")
	if err != nil {
		t.Fatalf("first InitializeProject() error = %v", err)
	}
	if first.State != "unlocked" || first.Project.Product != "retail" || first.Lock != nil {
		t.Fatalf("first status = %+v, want an unlocked retail project", first)
	}
	if _, err := os.Stat(filepath.Join(directory, projectLockFile)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("initial lock stat error = %v, want not exist", err)
	}

	before, err := os.ReadFile(filepath.Join(directory, projectFile))
	if err != nil {
		t.Fatal(err)
	}
	second, err := InitializeProject(ctx, directory, "retail")
	if err != nil {
		t.Fatalf("idempotent InitializeProject() error = %v", err)
	}
	if second != first {
		t.Fatalf("idempotent status = %+v, want %+v", second, first)
	}
	after, err := os.ReadFile(filepath.Join(directory, projectFile))
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != string(before) {
		t.Fatal("idempotent initialization rewrote the declaration")
	}

	foreign := t.TempDir()
	foreignPath := filepath.Join(foreign, projectFile)
	foreignBytes := []byte(`{"schema":"foreign.project.v9","product":"retail"}`)
	if err := os.WriteFile(foreignPath, foreignBytes, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := InitializeProject(ctx, foreign, "retail"); !errors.Is(err, ErrProjectFormat) {
		t.Fatalf("foreign declaration error = %v, want %v", err, ErrProjectFormat)
	}
	got, err := os.ReadFile(foreignPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(foreignBytes) {
		t.Fatal("foreign declaration was overwritten")
	}

	conflict := t.TempDir()
	if _, err := InitializeProject(ctx, conflict, "classic"); err != nil {
		t.Fatal(err)
	}
	conflictBefore, err := os.ReadFile(filepath.Join(conflict, projectFile))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := InitializeProject(ctx, conflict, "retail"); !errors.Is(err, ErrProjectConflict) {
		t.Fatalf("foreign product error = %v, want %v", err, ErrProjectConflict)
	}
	conflictAfter, err := os.ReadFile(filepath.Join(conflict, projectFile))
	if err != nil {
		t.Fatal(err)
	}
	if string(conflictAfter) != string(conflictBefore) {
		t.Fatal("foreign product declaration was overwritten")
	}
}

func TestLockProjectValidatesProductAndFixedPinHash(t *testing.T) {
	ctx := context.Background()

	productDirectory := t.TempDir()
	if _, err := InitializeProject(ctx, productDirectory, "retail"); err != nil {
		t.Fatal(err)
	}
	classicWorkspace := testWorkspace(t)
	classicPin := testPin(t, classicWorkspace.Root(), "classic", "a")
	if _, err := LockProject(ctx, classicWorkspace.Root(), productDirectory, classicPin.ID); !errors.Is(err, ErrProjectConflict) {
		t.Fatalf("product mismatch error = %v, want %v", err, ErrProjectConflict)
	}
	if _, err := os.Stat(filepath.Join(productDirectory, projectLockFile)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("product mismatch lock stat error = %v, want not exist", err)
	}

	hashDirectory := t.TempDir()
	if _, err := InitializeProject(ctx, hashDirectory, "retail"); err != nil {
		t.Fatal(err)
	}
	retailWorkspace := testWorkspace(t)
	pin := testPin(t, retailWorkspace.Root(), "retail", "b")
	lockStatus, err := LockProject(ctx, retailWorkspace.Root(), hashDirectory, pin.ID)
	if err != nil {
		t.Fatalf("initial LockProject() error = %v", err)
	}
	lockBefore, err := os.ReadFile(filepath.Join(hashDirectory, projectLockFile))
	if err != nil {
		t.Fatal(err)
	}

	mutated := pin
	mutated.Source.ExactCommit = strings.Repeat("c", 40)
	raw, err := json.Marshal(mutated)
	if err != nil {
		t.Fatal(err)
	}
	if err := replacePinnedDocument(ctx, retailWorkspace.Root(), pin.ID, raw); err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, retailWorkspace.Root(), hashDirectory, pin.ID); err == nil || !strings.Contains(err.Error(), "selection.pin_integrity") {
		t.Fatalf("fixed-pin hash error = %v, want selection.pin_integrity", err)
	}
	lockAfter, err := os.ReadFile(filepath.Join(hashDirectory, projectLockFile))
	if err != nil {
		t.Fatal(err)
	}
	if string(lockAfter) != string(lockBefore) {
		t.Fatalf("invalid fixed pin replaced lock: before %q after %q", lockBefore, lockAfter)
	}
	if lockStatus.Lock == nil || lockStatus.Lock.Selection.ID != pin.ID {
		t.Fatalf("initial lock status = %+v", lockStatus)
	}
}

func TestLockProjectReplacesValidPreviousLockAfterDeclarationEdit(t *testing.T) {
	ctx := context.Background()
	workspace := testWorkspace(t)
	retailPin := testPin(t, workspace.Root(), "retail", "a")
	classicPin := testPin(t, workspace.Root(), "classic", "b")

	directory := t.TempDir()
	if _, err := InitializeProject(ctx, directory, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, workspace.Root(), directory, retailPin.ID); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, projectFile), []byte(`{"schema":"lycheedev.project.v1","product":"classic"}`), 0600); err != nil {
		t.Fatal(err)
	}
	status, err := LockProject(ctx, workspace.Root(), directory, classicPin.ID)
	if err != nil {
		t.Fatalf("LockProject() after deliberate declaration edit = %v", err)
	}
	if status.Project.Product != "classic" || status.Lock == nil || status.Lock.Selection.ID != classicPin.ID {
		t.Fatalf("replacement status = %+v", status)
	}

	invalidDirectory := t.TempDir()
	if _, err := InitializeProject(ctx, invalidDirectory, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, workspace.Root(), invalidDirectory, retailPin.ID); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(invalidDirectory, projectFile), []byte(`{"schema":"lycheedev.project.v1","product":"classic"}`), 0600); err != nil {
		t.Fatal(err)
	}
	corrupt := retailPin
	corruptSource := *retailPin.Source
	corruptSource.ExactCommit = strings.Repeat("c", 40)
	corrupt.Source = &corruptSource
	if err := os.WriteFile(filepath.Join(invalidDirectory, projectLockFile), []byte(marshalProjectLock(t, corrupt)), 0600); err != nil {
		t.Fatal(err)
	}
	lockBefore, err := os.ReadFile(filepath.Join(invalidDirectory, projectLockFile))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, workspace.Root(), invalidDirectory, classicPin.ID); err == nil || !errors.Is(err, ErrProjectFormat) || !strings.Contains(err.Error(), "selection.pin_integrity") {
		t.Fatalf("corrupt previous lock error = %v, want wrapped ErrProjectFormat selection.pin_integrity", err)
	}
	lockAfter, err := os.ReadFile(filepath.Join(invalidDirectory, projectLockFile))
	if err != nil {
		t.Fatal(err)
	}
	if string(lockAfter) != string(lockBefore) {
		t.Fatal("corrupt previous lock was replaced")
	}
}

func TestLoadProjectSelectionImportsDerivedHeadAcrossWorkspaces(t *testing.T) {
	ctx := context.Background()
	sourceWorkspace := testWorkspace(t)
	parent := testPin(t, sourceWorkspace.Root(), "retail", "d")
	child, err := vault.WriteMetadata(ctx, sourceWorkspace.Root(), func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).PinSelection(ctx, SelectionSpec{
			Parent: parent.ID,
			Data: &DataPin{
				Product:          "retail",
				Region:           "cn",
				FullBuild:        "12.1.0.69875",
				BuildConfig:      strings.Repeat("e", 32),
				CDNConfig:        strings.Repeat("f", 32),
				Language:         "zhCN",
				DefinitionCommit: strings.Repeat("1", 40),
			},
		})
	})
	if err != nil {
		t.Fatal(err)
	}
	if child.Parent != parent.ID || child.Source == nil || child.Data == nil || child.ID == parent.ID {
		t.Fatalf("derived child = %+v", child)
	}

	projectDirectory := t.TempDir()
	if _, err := InitializeProject(ctx, projectDirectory, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := LockProject(ctx, sourceWorkspace.Root(), projectDirectory, child.ID); err != nil {
		t.Fatal(err)
	}

	destinationWorkspace := testWorkspace(t)
	if err := os.RemoveAll(sourceWorkspace.Root()); err != nil {
		t.Fatalf("remove source cache: %v", err)
	}
	status, err := LoadProjectSelection(ctx, destinationWorkspace.Root(), projectDirectory)
	if err != nil {
		t.Fatalf("LoadProjectSelection() error = %v", err)
	}
	if status.Lock == nil || status.Lock.Selection.ID != child.ID || status.Lock.Selection.Parent != parent.ID {
		t.Fatalf("imported status = %+v", status)
	}
	imported, err := vault.ReadWorkspace(ctx, destinationWorkspace.Root(), func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).ReadPinnedSet(ctx, child.ID)
	})
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(imported, child) {
		t.Fatalf("imported child = %+v, want %+v", imported, child)
	}
	if _, err := vault.ReadWorkspace(ctx, destinationWorkspace.Root(), func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).ReadPinnedSet(ctx, parent.ID)
	}); !errors.Is(err, vault.ErrMissingRecord) {
		t.Fatalf("parent unexpectedly imported: %v", err)
	}
}

func TestFindProjectUsesNearestDeclarationIncludingMalformedOne(t *testing.T) {
	root := t.TempDir()
	outer := filepath.Join(root, "outer")
	inner := filepath.Join(outer, "inner")
	if err := os.MkdirAll(inner, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := InitializeProject(context.Background(), outer, "retail"); err != nil {
		t.Fatal(err)
	}

	found, err := FindProject(inner)
	if err != nil || found != mustAbsProject(t, outer) {
		t.Fatalf("FindProject() = %q, %v; want outer %q", found, err, mustAbsProject(t, outer))
	}

	nearer := filepath.Join(inner, projectFile)
	if err := os.WriteFile(nearer, []byte(`{"schema":"not-a-project","product":"retail"}`), 0600); err != nil {
		t.Fatal(err)
	}
	found, err = FindProject(inner)
	if err != nil || found != mustAbsProject(t, inner) {
		t.Fatalf("malformed-nearer FindProject() = %q, %v; want inner %q", found, err, mustAbsProject(t, inner))
	}
	if _, err := InspectProject(context.Background(), found); !errors.Is(err, ErrProjectFormat) {
		t.Fatalf("malformed nearer InspectProject() error = %v, want %v", err, ErrProjectFormat)
	}
}

func TestInspectProjectIsReadOnlyAndCreatesNoDirectories(t *testing.T) {
	directory := t.TempDir()
	if err := os.WriteFile(filepath.Join(directory, projectFile), []byte(`{"schema":"lycheedev.project.v1","product":"retail"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := InspectProject(context.Background(), directory); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Name() != projectFile {
		t.Fatalf("read-only inspection changed directory: %v", entries)
	}
}

func TestInitializeProjectConcurrentCreationPublishesOneDeclaration(t *testing.T) {
	directory := t.TempDir()
	results := make(chan error, 16)
	var group sync.WaitGroup
	for worker := 0; worker < 16; worker++ {
		group.Go(func() {
			status, err := InitializeProject(context.Background(), directory, "retail")
			if err != nil {
				results <- err
				return
			}
			if status.State != "unlocked" || status.Project.Product != "retail" || status.Lock != nil {
				results <- fmt.Errorf("unexpected concurrent init status: %+v", status)
			}
		})
	}
	group.Wait()
	close(results)
	for err := range results {
		t.Error(err)
	}

	status, err := InspectProject(context.Background(), directory)
	if err != nil {
		t.Fatal(err)
	}
	if status.State != "unlocked" || status.Project.Product != "retail" || status.Lock != nil {
		t.Fatalf("published status = %+v", status)
	}
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".lycheedev-project-") {
			t.Fatalf("temporary project file left behind: %s", entry.Name())
		}
	}
}

func TestProjectFilesRejectUnknownSchemasDuplicateNestedFieldsAndOversizeInput(t *testing.T) {
	tests := []struct {
		name      string
		configure func(t *testing.T, directory string)
	}{
		{
			name: "unknown project schema",
			configure: func(t *testing.T, directory string) {
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v2","product":"retail"}`)
			},
		},
		{
			name: "unknown lock schema",
			configure: func(t *testing.T, directory string) {
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v1","product":"retail"}`)
				writeProjectTestFile(t, directory, projectLockFile, `{"schema":"lycheedev.project-lock.v2","selection":{}}`)
			},
		},
		{
			name: "unknown nested field",
			configure: func(t *testing.T, directory string) {
				pin := standaloneTestPin(t)
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v1","product":"retail"}`)
				raw := marshalProjectLock(t, pin)
				raw = strings.Replace(raw, `"repository":"fixture"`, `"repository":"fixture","unexpected":true`, 1)
				writeProjectTestFile(t, directory, projectLockFile, raw)
			},
		},
		{
			name: "duplicate nested field",
			configure: func(t *testing.T, directory string) {
				pin := standaloneTestPin(t)
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v1","product":"retail"}`)
				raw := marshalProjectLock(t, pin)
				raw = strings.Replace(raw, `"product":"retail","requestedRef"`, `"product":"retail","product":"retail","requestedRef"`, 1)
				writeProjectTestFile(t, directory, projectLockFile, raw)
			},
		},
		{
			name: "case-folded duplicate nested field",
			configure: func(t *testing.T, directory string) {
				pin := standaloneTestPin(t)
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v1","product":"retail"}`)
				raw := marshalProjectLock(t, pin)
				raw = strings.Replace(raw, `"product":"retail","requestedRef"`, `"PRODUCT":"retail","product":"retail","requestedRef"`, 1)
				writeProjectTestFile(t, directory, projectLockFile, raw)
			},
		},
		{
			name: "invalid locked pin hash",
			configure: func(t *testing.T, directory string) {
				pin := standaloneTestPin(t)
				corrupt := pin
				corruptSource := *pin.Source
				corruptSource.ExactCommit = strings.Repeat("b", 40)
				corrupt.Source = &corruptSource
				writeProjectTestFile(t, directory, projectFile, `{"schema":"lycheedev.project.v1","product":"retail"}`)
				writeProjectTestFile(t, directory, projectLockFile, marshalProjectLock(t, corrupt))
			},
		},
		{
			name: "oversize declaration",
			configure: func(t *testing.T, directory string) {
				writeProjectTestFile(t, directory, projectFile, strings.Repeat("x", 65537))
			},
		},
		{
			name: "invalid utf8",
			configure: func(t *testing.T, directory string) {
				writeProjectTestBytes(t, directory, projectFile, append([]byte(`{"schema":"lycheedev.project.v1","product":"`), 0xff, '"', '}'))
			},
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			directory := t.TempDir()
			test.configure(t, directory)
			if _, err := InspectProject(context.Background(), directory); !errors.Is(err, ErrProjectFormat) {
				t.Fatalf("InspectProject() error = %v, want %v", err, ErrProjectFormat)
			}
		})
	}
}

func TestProjectLockReplacementIsAtomicForConcurrentReaders(t *testing.T) {
	ctx := context.Background()
	directory := t.TempDir()
	if _, err := InitializeProject(ctx, directory, "retail"); err != nil {
		t.Fatal(err)
	}
	workspace := testWorkspace(t)
	first := testPin(t, workspace.Root(), "retail", "a")
	second := testPin(t, workspace.Root(), "retail", "b")
	known := map[string]PinnedSet{first.ID: first, second.ID: second}

	errorsSeen := make(chan error, 4096)
	var group sync.WaitGroup
	for writer := 0; writer < 4; writer++ {
		writer := writer
		group.Go(func() {
			for iteration := 0; iteration < 30; iteration++ {
				id := first.ID
				if (writer+iteration)%2 == 1 {
					id = second.ID
				}
				if _, err := LockProject(ctx, workspace.Root(), directory, id); err != nil {
					errorsSeen <- fmt.Errorf("writer %d iteration %d: %w", writer, iteration, err)
				}
			}
		})
	}
	for reader := 0; reader < 8; reader++ {
		reader := reader
		group.Go(func() {
			for iteration := 0; iteration < 100; iteration++ {
				status, err := InspectProject(ctx, directory)
				if err != nil {
					errorsSeen <- fmt.Errorf("reader %d iteration %d: %w", reader, iteration, err)
					continue
				}
				if status.State == "unlocked" {
					if status.Lock != nil {
						errorsSeen <- fmt.Errorf("reader %d saw unlocked status with a lock", reader)
					}
					continue
				}
				if status.State != "locked" || status.Lock == nil {
					errorsSeen <- fmt.Errorf("reader %d saw incomplete status %+v", reader, status)
					continue
				}
				want, ok := known[status.Lock.Selection.ID]
				if !ok || !reflect.DeepEqual(status.Lock.Selection, want) {
					errorsSeen <- fmt.Errorf("reader %d saw incomplete pin %+v", reader, status.Lock.Selection)
				}
			}
		})
	}
	group.Wait()
	close(errorsSeen)
	for err := range errorsSeen {
		t.Error(err)
	}
	if _, err := InspectProject(ctx, directory); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".lycheedev-project-") {
			t.Fatalf("temporary project file left behind: %s", entry.Name())
		}
	}
}

func TestCanceledProjectWritesLeaveExistingFilesUnchanged(t *testing.T) {
	ctx := context.Background()
	directory := t.TempDir()
	if _, err := InitializeProject(ctx, directory, "retail"); err != nil {
		t.Fatal(err)
	}
	projectPath := filepath.Join(directory, projectFile)
	projectBefore, err := os.ReadFile(projectPath)
	if err != nil {
		t.Fatal(err)
	}
	canceled, cancel := context.WithCancel(ctx)
	cancel()
	if err := writeProjectFile(canceled, directory, projectFile, Project{Schema: "lycheedev.project.v1", Product: "classic"}); !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled declaration write error = %v, want context canceled", err)
	}
	projectAfter, err := os.ReadFile(projectPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(projectAfter) != string(projectBefore) {
		t.Fatal("canceled declaration write replaced the existing file")
	}

	workspace := testWorkspace(t)
	first := testPin(t, workspace.Root(), "retail", "d")
	second := testPin(t, workspace.Root(), "retail", "e")
	if _, err := LockProject(ctx, workspace.Root(), directory, first.ID); err != nil {
		t.Fatal(err)
	}
	lockPath := filepath.Join(directory, projectLockFile)
	lockBefore, err := os.ReadFile(lockPath)
	if err != nil {
		t.Fatal(err)
	}
	canceled, cancel = context.WithCancel(ctx)
	cancel()
	if _, err := LockProject(canceled, workspace.Root(), directory, second.ID); !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled LockProject() error = %v, want context canceled", err)
	}
	lockAfter, err := os.ReadFile(lockPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(lockAfter) != string(lockBefore) {
		t.Fatal("canceled lock write replaced the existing file")
	}
}

func testWorkspace(t *testing.T) *vault.Store {
	t.Helper()
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	return store
}

func testPin(t *testing.T, workspace, product, commitLetter string) PinnedSet {
	t.Helper()
	return testPinWithContext(t, workspace, product, commitLetter, "")
}

func testPinWithContext(t *testing.T, workspace, product, commitLetter, parent string) PinnedSet {
	t.Helper()
	pin, err := vault.WriteMetadata(context.Background(), workspace, func(_ *vault.Store, m *vault.Metadata) (PinnedSet, error) {
		return OpenPinner(m).PinSelection(context.Background(), SelectionSpec{
			Parent: parent,
			Source: &SourcePin{
				Repository:     "fixture",
				Product:        product,
				RequestedRef:   "main",
				ExactCommit:    strings.Repeat(commitLetter, 40),
				ParserRevision: "parser-v1",
			},
		})
	})
	if err != nil {
		t.Fatal(err)
	}
	return pin
}

func standaloneTestPin(t *testing.T) PinnedSet {
	t.Helper()
	workspace := testWorkspace(t)
	return testPin(t, workspace.Root(), "retail", "a")
}

func replacePinnedDocument(ctx context.Context, workspace, id string, value []byte) error {
	_, err := vault.WriteMetadata(ctx, workspace, func(_ *vault.Store, m *vault.Metadata) (struct{}, error) {
		doc, err := m.ReadDocument(ctx, "pin/"+id)
		if err != nil {
			return struct{}{}, err
		}
		return struct{}{}, m.CommitDocuments(ctx, vault.Mutation{Key: doc.Key, ExpectedGeneration: doc.Generation, Value: value})
	})
	return err
}

func marshalProjectLock(t *testing.T, pin PinnedSet) string {
	t.Helper()
	raw, err := json.Marshal(ProjectLock{Schema: "lycheedev.project-lock.v1", Selection: pin})
	if err != nil {
		t.Fatal(err)
	}
	return string(raw)
}

func writeProjectTestFile(t *testing.T, directory, name, contents string) {
	t.Helper()
	writeProjectTestBytes(t, directory, name, []byte(contents))
}

func writeProjectTestBytes(t *testing.T, directory, name string, contents []byte) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(directory, name), contents, 0600); err != nil {
		t.Fatal(err)
	}
}

func mustAbsProject(t *testing.T, path string) string {
	t.Helper()
	abs, err := filepath.Abs(path)
	if err != nil {
		t.Fatal(err)
	}
	return abs
}
