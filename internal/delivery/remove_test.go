package delivery

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestRemovalArchivesOwnedContent(t *testing.T) {
	target, receipt := installedFixture(t)
	archive := filepath.Join(t.TempDir(), "recovered-skill")
	result, err := RemoveInstallation(context.Background(), target, archive, "skill")
	if err != nil || result.State != "archived" || !sameReceipt(*result.Receipt, receipt) {
		t.Fatalf("%+v %v", result, err)
	}
	if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("target: %v", err)
	}
	assessment, err := InspectInstallation(context.Background(), archive, "skill")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("archive: %+v %v", assessment, err)
	}
	result, err = RemoveInstallation(context.Background(), target, archive, "skill")
	if err != nil || result.State != "absent" {
		t.Fatalf("retry: %+v %v", result, err)
	}
}

func TestRemovalDoesNotOverwriteOrRemoveEdits(t *testing.T) {
	for _, kind := range []string{"edited", "archive-exists", "discovery-parent", "nested", "unmanaged"} {
		t.Run(kind, func(t *testing.T) {
			target, _ := installedFixture(t)
			archive := filepath.Join(t.TempDir(), "archive")
			switch kind {
			case "edited":
				if err := os.WriteFile(filepath.Join(target, "SKILL.md"), []byte("edit"), 0600); err != nil {
					t.Fatal(err)
				}
			case "archive-exists":
				if err := os.Mkdir(archive, 0700); err != nil {
					t.Fatal(err)
				}
			case "discovery-parent":
				archive = filepath.Join(filepath.Dir(target), "archive")
			case "nested":
				archive = filepath.Join(target, "archive")
			case "unmanaged":
				if err := os.Remove(filepath.Join(target, installationMarker)); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := RemoveInstallation(context.Background(), target, archive, "skill"); err == nil {
				t.Fatal("expected refusal")
			}
			if _, err := os.Stat(filepath.Join(target, "SKILL.md")); err != nil {
				t.Fatalf("lost source: %v", err)
			}
		})
	}
}
