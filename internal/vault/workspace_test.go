package vault

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInitializeAndOpenStoreAreIdempotent(t *testing.T) {
	parent := t.TempDir()
	root := filepath.Join(parent, "workspace")

	first, err := Initialize(context.Background(), root)
	if err != nil {
		t.Fatalf("Initialize() error = %v", err)
	}
	if first.Root() != mustAbs(t, root) {
		t.Fatalf("Root() = %q, want %q", first.Root(), mustAbs(t, root))
	}
	if first.Identity().Schema != WorkspaceSchema {
		t.Fatalf("identity schema = %q, want %q", first.Identity().Schema, WorkspaceSchema)
	}
	if len(first.Identity().WorkspaceID) != 32 {
		t.Fatalf("workspace ID length = %d, want 32", len(first.Identity().WorkspaceID))
	}
	if first.Identity().CreatedAt.IsZero() {
		t.Fatal("identity CreatedAt is zero")
	}
	if _, err := os.Stat(filepath.Join(root, "workspace.json")); err != nil {
		t.Fatalf("workspace marker is missing: %v", err)
	}
	for _, name := range []string{"state", "pins", "mirrors", "blobs", "indexes", "cache", "runs", "captures", "locks", "tmp"} {
		if info, err := os.Stat(filepath.Join(root, name)); err != nil || !info.IsDir() {
			t.Fatalf("workspace directory %q is missing or not a directory: %v", name, err)
		}
	}

	markerBefore, err := os.ReadFile(filepath.Join(root, "workspace.json"))
	if err != nil {
		t.Fatalf("read initial marker: %v", err)
	}
	opened, err := OpenStore(root)
	if err != nil {
		t.Fatalf("OpenStore() error = %v", err)
	}
	if opened.Identity() != first.Identity() {
		t.Fatalf("OpenStore() identity = %#v, want %#v", opened.Identity(), first.Identity())
	}

	reopened, err := Initialize(context.Background(), root)
	if err != nil {
		t.Fatalf("second Initialize() error = %v", err)
	}
	if reopened.Identity() != first.Identity() {
		t.Fatalf("second Initialize() identity = %#v, want %#v", reopened.Identity(), first.Identity())
	}
	markerAfter, err := os.ReadFile(filepath.Join(root, "workspace.json"))
	if err != nil {
		t.Fatalf("read marker after reinitialize: %v", err)
	}
	if string(markerAfter) != string(markerBefore) {
		t.Fatalf("reinitialize changed workspace marker")
	}
}

func TestInitializeAndOpenStoreLeaveLegacyDirectoryUntouched(t *testing.T) {
	root := filepath.Join(t.TempDir(), "legacy")
	if err := os.MkdirAll(root, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(root, "legacy-sentinel.txt")
	const contents = "legacy data must remain untouched\n"
	if err := os.WriteFile(sentinel, []byte(contents), 0600); err != nil {
		t.Fatal(err)
	}

	for name, open := range map[string]func() error{
		"OpenStore": func() error {
			_, err := OpenStore(root)
			return err
		},
		"Initialize": func() error {
			_, err := Initialize(context.Background(), root)
			return err
		},
	} {
		err := open()
		if !errors.Is(err, ErrLegacyWorkspace) {
			t.Errorf("%s() error = %v, want %v", name, err, ErrLegacyWorkspace)
		}
		got, readErr := os.ReadFile(sentinel)
		if readErr != nil {
			t.Errorf("%s() removed sentinel: %v", name, readErr)
		} else if string(got) != contents {
			t.Errorf("%s() changed sentinel to %q", name, got)
		}
		if _, statErr := os.Stat(filepath.Join(root, "workspace.json")); !errors.Is(statErr, os.ErrNotExist) {
			t.Errorf("%s() marker stat error = %v, want os.ErrNotExist", name, statErr)
		}
	}
}

func TestOpenStoreRejectsInvalidAndFutureMarkers(t *testing.T) {
	validID := strings.Repeat("a", 32)
	tests := []struct {
		name   string
		marker string
	}{
		{name: "malformed JSON", marker: "{"},
		{name: "wrong schema", marker: `{"schema":"other","workspaceId":"` + validID + `","createdAt":"2026-09-21T00:00:00Z"}`},
		{name: "future schema", marker: `{"schema":"lycheedev.workspace.v2","workspaceId":"` + validID + `","createdAt":"2026-09-21T00:00:00Z"}`},
		{name: "short ID", marker: `{"schema":"` + WorkspaceSchema + `","workspaceId":"abc","createdAt":"2026-09-21T00:00:00Z"}`},
		{name: "non-hex ID", marker: `{"schema":"` + WorkspaceSchema + `","workspaceId":"` + strings.Repeat("g", 32) + `","createdAt":"2026-09-21T00:00:00Z"}`},
		{name: "missing creation time", marker: `{"schema":"` + WorkspaceSchema + `","workspaceId":"` + validID + `"}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			root := t.TempDir()
			if err := os.WriteFile(filepath.Join(root, "workspace.json"), []byte(tt.marker), 0600); err != nil {
				t.Fatal(err)
			}
			_, err := OpenStore(root)
			if !errors.Is(err, ErrWorkspaceFormat) {
				t.Fatalf("OpenStore() error = %v, want %v", err, ErrWorkspaceFormat)
			}
		})
	}
}

func mustAbs(t *testing.T, path string) string {
	t.Helper()
	abs, err := filepath.Abs(path)
	if err != nil {
		t.Fatal(err)
	}
	return abs
}

func TestInitializeExistingEmptyDirectory(t *testing.T) {
	root := t.TempDir()
	store, err := Initialize(context.Background(), root)
	if err != nil {
		t.Fatal(err)
	}
	if store.Root() != mustAbs(t, root) {
		t.Fatal("wrong workspace")
	}
}
