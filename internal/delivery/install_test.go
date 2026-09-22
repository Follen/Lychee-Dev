package delivery

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

func TestFreshInstallationAndRetry(t *testing.T) {
	release, manifest := releaseFixture(t)
	parent := t.TempDir()
	target := filepath.Join(parent, "skill 安装")
	first, err := InstallFresh(context.Background(), release, target, "skill", manifest.Version)
	if err != nil {
		t.Fatal(err)
	}
	second, err := InstallFresh(context.Background(), release, target, "skill", manifest.Version)
	if err != nil || !sameReceipt(first, second) {
		t.Fatalf("retry: %+v %v", second, err)
	}
	assessment, err := InspectInstallation(context.Background(), target, "skill")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("%+v %v", assessment, err)
	}
	entries, err := os.ReadDir(parent)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".lycheedev-stage-") {
			t.Fatalf("leaked stage: %s", entry.Name())
		}
	}
}

func TestFreshInstallationPreservesUnmanagedAndEdits(t *testing.T) {
	for _, managed := range []bool{false, true} {
		release, manifest := releaseFixture(t)
		target := filepath.Join(t.TempDir(), "lycheedev")
		if managed {
			if _, err := InstallFresh(context.Background(), release, target, "skill", manifest.Version); err != nil {
				t.Fatal(err)
			}
		} else if err := os.Mkdir(target, 0700); err != nil {
			t.Fatal(err)
		}
		file := filepath.Join(target, "SKILL.md")
		if err := os.WriteFile(file, []byte("user content"), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := InstallFresh(context.Background(), release, target, "skill", manifest.Version); !errors.Is(err, ErrConflict) {
			t.Fatalf("conflict: %v", err)
		}
		content, err := os.ReadFile(file)
		if err != nil || string(content) != "user content" {
			t.Fatalf("modified: %q %v", content, err)
		}
	}
}

func TestFreshInstallationConcurrentRetries(t *testing.T) {
	release, manifest := releaseFixture(t)
	target := filepath.Join(t.TempDir(), "lycheedev")
	var workers sync.WaitGroup
	results := make(chan error, 4)
	for range 4 {
		workers.Add(1)
		go func() {
			defer workers.Done()
			_, err := InstallFresh(context.Background(), release, target, "skill", manifest.Version)
			results <- err
		}()
	}
	workers.Wait()
	close(results)
	for err := range results {
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestPublishDirectoryNeverReplacesExistingTarget(t *testing.T) {
	parent := t.TempDir()
	source, target := filepath.Join(parent, "source"), filepath.Join(parent, "target")
	if err := os.Mkdir(source, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(target, 0700); err != nil {
		t.Fatal(err)
	}
	if err := publishDirectory(source, target); err == nil {
		t.Fatal("replaced existing empty directory")
	}
	if _, err := os.Stat(source); err != nil {
		t.Fatal(err)
	}
}
