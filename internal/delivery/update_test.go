package delivery

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestUpdateManagedPlanReplaceRepeat(t *testing.T) {
	source, target, _, version := upgradeFixture(t)
	state := filepath.Join(t.TempDir(), "state")
	targets := []UpdateTarget{{"skill", target}, {"skill", target}}
	plan, err := UpdateManaged(context.Background(), source, version, state, targets, true)
	if err != nil || plan.Complete || len(plan.Targets) != 1 {
		t.Fatalf("plan: %+v %v", plan, err)
	}
	if _, err := os.Stat(state); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("plan wrote state: %v", err)
	}
	for range 2 {
		result, err := UpdateManaged(context.Background(), source, version, state, targets, false)
		if err != nil || !result.Complete || result.Targets[0].State != "current" {
			t.Fatalf("update: %+v %v", result, err)
		}
		status, err := InspectInstallation(context.Background(), target, "skill")
		if err != nil || status.State != "managed" || status.Receipt.Commit != strings.Repeat("b", 40) {
			t.Fatalf("status: %+v %v", status, err)
		}
		if _, err := os.Stat(plan.Targets[0].Recovery); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("old files retained: %v", err)
		}
	}
	remembered, err := ReadUpdateTargets(state)
	if err != nil || len(remembered) != 1 || remembered[0].Path != target {
		t.Fatalf("inventory: %+v %v", remembered, err)
	}
}

func TestUpdatePreflightsAllBeforeFirstMutation(t *testing.T) {
	source, target, _, version := upgradeFixture(t)
	conflict := filepath.Join(t.TempDir(), "lycheedev")
	if err := os.Mkdir(conflict, 0700); err != nil {
		t.Fatal(err)
	}
	state := filepath.Join(t.TempDir(), "state")
	result, err := UpdateManaged(context.Background(), source, version, state, []UpdateTarget{{"skill", target}, {"skill", conflict}}, false)
	if !errors.Is(err, ErrConflict) || result.Complete {
		t.Fatalf("conflict: %+v %v", result, err)
	}
	status, err := InspectInstallation(context.Background(), target, "skill")
	if err != nil || status.Receipt.Commit != strings.Repeat("a", 40) {
		t.Fatal("changed earlier target before later conflict")
	}
	if _, err := os.Stat(state); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("conflict wrote state: %v", err)
	}
}

func TestUpdateRecoversOldMovedCheckpoint(t *testing.T) {
	source, target, _, version := upgradeFixture(t)
	state := t.TempDir()
	targets := []UpdateTarget{{"skill", target}}
	plan, err := UpdateManaged(context.Background(), source, version, state, targets, true)
	if err != nil {
		t.Fatal(err)
	}
	archive := plan.Targets[0].Recovery
	if err := os.MkdirAll(filepath.Dir(archive), 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_, err = UpgradeInstallation(cancelAfterOldMove{ctx, archive, cancel}, source, target, archive, "skill", version)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("checkpoint: %v", err)
	}
	result, err := UpdateManaged(context.Background(), source, version, state, targets, false)
	if err != nil || !result.Complete {
		t.Fatalf("recovery: %+v %v", result, err)
	}
}

func TestUpdateRecoversInterruptedOldFileDeletion(t *testing.T) {
	for _, point := range []string{"partial-old-tree", "archive-removed"} {
		t.Run(point, func(t *testing.T) {
			source, target, _, version := upgradeFixture(t)
			state, targets := t.TempDir(), []UpdateTarget{{"skill", target}}
			plan, err := UpdateManaged(context.Background(), source, version, state, targets, true)
			if err != nil {
				t.Fatal(err)
			}
			archive := plan.Targets[0].Recovery
			if err := os.MkdirAll(filepath.Dir(archive), 0700); err != nil {
				t.Fatal(err)
			}
			if _, err := UpgradeInstallation(context.Background(), source, target, archive, "skill", version); err != nil {
				t.Fatal(err)
			}
			raw, err := os.ReadFile(filepath.Join(archive, "replacement.json"))
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(archive+".cleanup.json", raw, 0600); err != nil {
				t.Fatal(err)
			}
			if point == "partial-old-tree" {
				err = os.Remove(filepath.Join(archive, "previous", "SKILL.md"))
			} else {
				err = os.RemoveAll(archive)
			}
			if err != nil {
				t.Fatal(err)
			}
			result, err := UpdateManaged(context.Background(), source, version, state, targets, false)
			if err != nil || !result.Complete {
				t.Fatalf("cleanup recovery: %+v %v", result, err)
			}
			if _, err := os.Stat(archive + ".cleanup.json"); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("cleanup intent left behind: %v", err)
			}
		})
	}
}

func TestUpdateRejectsChangedAndContainedTargets(t *testing.T) {
	for _, change := range []string{"edited", "nested-state", "redirected"} {
		t.Run(change, func(t *testing.T) {
			source, target, _, version := upgradeFixture(t)
			state := filepath.Join(t.TempDir(), "state")
			switch change {
			case "edited":
				if err := os.WriteFile(filepath.Join(target, "SKILL.md"), []byte("my edits"), 0600); err != nil {
					t.Fatal(err)
				}
			case "nested-state":
				state = filepath.Join(target, "state")
			case "redirected":
				link := filepath.Join(t.TempDir(), "lycheedev")
				if runtime.GOOS == "windows" {
					command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
					command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+target)
					if output, err := command.CombinedOutput(); err != nil {
						t.Fatalf("junction: %v %s", err, output)
					}
				} else if err := os.Symlink(target, link); err != nil {
					t.Fatal(err)
				}
				defer os.Remove(link)
				target = link
			}
			result, err := UpdateManaged(context.Background(), source, version, state, []UpdateTarget{{"skill", target}}, false)
			if err == nil || result.Complete {
				t.Fatalf("accepted %s: %+v", change, result)
			}
			if _, err := os.Stat(state); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("failed preflight wrote state: %v", err)
			}
		})
	}
}
