//go:build windows

package luals

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestDirectoryWatchCloseJoinsWithoutFilesystemEvents(t *testing.T) {
	stop, err := watchWorkspace(t.TempDir(), func() {})
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	go func() { stop(); close(done) }()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("watcher did not join after cancellation")
	}
	stop()
}
func TestDirectoryWatchInvalidatesOnRestoredSource(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "a.lua")
	if err := os.WriteFile(path, []byte("return 1\n"), 0600); err != nil {
		t.Fatal(err)
	}
	invalidated := make(chan struct{}, 1)
	stop, err := watchWorkspace(dir, func() { invalidated <- struct{}{} })
	if err != nil {
		t.Fatal(err)
	}
	defer stop()
	if err = os.WriteFile(path, []byte("return 2\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte("return 1\n"), 0600); err != nil {
		t.Fatal(err)
	}
	select {
	case <-invalidated:
	case <-time.After(2 * time.Second):
		t.Fatal("restored edit did not invalidate session")
	}
}

func TestSessionArtifactWatcherRetiresRestoredSealedFiles(t *testing.T) {
	for _, name := range []string{"config.json", "definitions/api.d.lua"} {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, filepath.FromSlash(name))
			if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
				t.Fatal(err)
			}
			original := []byte("sealed bytes\n")
			if err := os.WriteFile(path, original, 0600); err != nil {
				t.Fatal(err)
			}
			retired := make(chan struct{}, 1)
			stop, err := watchSessionArtifacts(dir, func() { retired <- struct{}{} })
			if err != nil {
				t.Fatal(err)
			}
			defer stop()
			if err := os.WriteFile(path, []byte("modified bytes\n"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, original, 0600); err != nil {
				t.Fatal(err)
			}
			select {
			case <-retired:
			case <-time.After(2 * time.Second):
				t.Fatal("restoring sealed bytes retained the old session")
			}
		})
	}
}
