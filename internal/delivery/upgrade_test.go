package delivery

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func upgradeFixture(t *testing.T) (string, string, string, string) {
	t.Helper()
	source, release := releaseFixture(t)
	target := filepath.Join(t.TempDir(), "lycheedev")
	if _, err := InstallFresh(context.Background(), source, target, "skill", release.Version); err != nil {
		t.Fatal(err)
	}
	release.Commit = strings.Repeat("b", 40)
	writeRelease(t, source, release)
	archive := filepath.Join(t.TempDir(), "upgrade")
	return source, target, archive, release.Version
}

func TestUpgradeAndCompletedResume(t *testing.T) {
	source, target, archive, version := upgradeFixture(t)
	result, err := UpgradeInstallation(context.Background(), source, target, archive, "skill", version)
	if err != nil || result.Receipt.Commit != strings.Repeat("b", 40) {
		t.Fatalf("%+v %v", result, err)
	}
	old, err := InspectInstallation(context.Background(), filepath.Join(archive, "previous"), "skill")
	if err != nil || old.State != "managed" || old.Receipt.Commit != strings.Repeat("a", 40) {
		t.Fatalf("%+v %v", old, err)
	}
	resumed, err := ResumeUpgrade(context.Background(), target, archive, "skill")
	if err != nil || !sameReceipt(result.Receipt, resumed.Receipt) {
		t.Fatalf("%+v %v", resumed, err)
	}
	if _, err := ResumeUpgrade(context.Background(), target, archive, "addon"); !errors.Is(err, ErrInstallation) {
		t.Fatalf("component mismatch: %v", err)
	}
}

func TestUpgradeResumesFilesystemCheckpoints(t *testing.T) {
	for _, checkpoint := range []string{"prepared", "old-moved", "tampered-next", "unexpected-target"} {
		t.Run(checkpoint, func(t *testing.T) {
			source, target, archive, version := upgradeFixture(t)
			if _, err := UpgradeInstallation(context.Background(), source, target, archive, "skill", version); err != nil {
				t.Fatal(err)
			}
			if err := publishDirectory(target, filepath.Join(archive, "next")); err != nil {
				t.Fatal(err)
			}
			if checkpoint == "prepared" {
				if err := publishDirectory(filepath.Join(archive, "previous"), target); err != nil {
					t.Fatal(err)
				}
			}
			if checkpoint == "tampered-next" {
				if err := os.WriteFile(filepath.Join(archive, "next", "SKILL.md"), []byte("tampered"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			if checkpoint == "unexpected-target" {
				if err := os.Mkdir(target, 0700); err != nil {
					t.Fatal(err)
				}
			}
			_, err := ResumeUpgrade(context.Background(), target, archive, "skill")
			if checkpoint == "tampered-next" || checkpoint == "unexpected-target" {
				if !errors.Is(err, ErrConflict) {
					t.Fatalf("expected conflict: %v", err)
				}
			} else if err != nil {
				t.Fatal(err)
			}
			old, err := InspectInstallation(context.Background(), filepath.Join(archive, "previous"), "skill")
			if err != nil || old.State != "managed" {
				t.Fatalf("lost backup: %+v %v", old, err)
			}
		})
	}
}

type cancelAfterOldMove struct {
	context.Context
	archive string
	cancel  context.CancelFunc
}

func (c cancelAfterOldMove) Err() error {
	if _, err := os.Stat(filepath.Join(c.archive, "previous")); err == nil {
		c.cancel()
	}
	return c.Context.Err()
}

func TestUpgradeCancellationAfterOldMove(t *testing.T) {
	source, target, archive, version := upgradeFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_, err := UpgradeInstallation(cancelAfterOldMove{ctx, archive, cancel}, source, target, archive, "skill", version)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel: %v", err)
	}
	if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("checkpoint not reached: %v", err)
	}
	// Recovery consumes the prepared tree, not the original distribution.
	if err := os.Remove(filepath.Join(source, "release.json")); err != nil {
		t.Fatal(err)
	}
	if _, err := ResumeUpgrade(context.Background(), target, archive, "skill"); err != nil {
		t.Fatal(err)
	}
}

type cancelUpgradePreparation struct {
	context.Context
	parent, checkpoint string
	cancel             context.CancelFunc
}

func (c cancelUpgradePreparation) Err() error {
	dirs, _ := filepath.Glob(filepath.Join(c.parent, ".lycheedev-prepare-*"))
	for _, dir := range dirs {
		path := dir
		if c.checkpoint == "next" {
			path = filepath.Join(dir, "next", installationMarker)
		}
		if c.checkpoint == "journal" {
			path = filepath.Join(dir, "replacement.json")
		}
		if _, err := os.Stat(path); err == nil {
			c.cancel()
		}
	}
	return c.Context.Err()
}
func TestUpgradePreparationInterruptionLeavesOriginalRetryable(t *testing.T) {
	for _, checkpoint := range []string{"created", "next", "journal"} {
		t.Run(checkpoint, func(t *testing.T) {
			source, target, archive, version := upgradeFixture(t)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			_, err := UpgradeInstallation(cancelUpgradePreparation{ctx, filepath.Dir(archive), checkpoint, cancel}, source, target, archive, "skill", version)
			if !errors.Is(err, context.Canceled) {
				t.Fatalf("checkpoint %s: %v", checkpoint, err)
			}
			if _, err := os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("partial archive published: %v", err)
			}
			before, err := InspectInstallation(context.Background(), target, "skill")
			if err != nil || before.State != "managed" || before.Receipt.Commit != strings.Repeat("a", 40) {
				t.Fatalf("original changed: %+v %v", before, err)
			}
			result, err := UpgradeInstallation(context.Background(), source, target, archive, "skill", version)
			if err != nil || result.Receipt.Commit != strings.Repeat("b", 40) {
				t.Fatalf("original retry: %+v %v", result, err)
			}
		})
	}
}
