package delivery_test

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
)

func addonUpdateFixture(t *testing.T) (release, client, state string) {
	t.Helper()
	release = testkit.Release(t, "")
	client = testkit.CanonicalPath(t, testkit.Client(t, "flavor"))
	state = t.TempDir()
	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(filepath.Join(release, "release.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest delivery.Release
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.Commit = strings.Repeat("b", 40)
	raw, err = json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(release, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	return release, client, state
}

type cancelAddonUpdateAfterMove struct {
	context.Context
	archive string
	cancel  context.CancelFunc
}

func (c cancelAddonUpdateAfterMove) Err() error {
	if _, err := os.Stat(filepath.Join(c.archive, "previous")); err == nil {
		c.cancel()
	}
	return c.Context.Err()
}

func TestAddonUpdateRootAndClientShareRecoveryIdentity(t *testing.T) {
	release, client, state := addonUpdateFixture(t)
	var recovery string
	for _, targets := range [][]delivery.UpdateTarget{
		{{Component: "addon", Path: filepath.Dir(client)}},
		{{Component: "addon", Path: client}},
		{{Component: "addon", Path: filepath.Dir(client)}, {Component: "addon", Path: client}},
		{{Component: "addon", Path: client}, {Component: "addon", Path: filepath.Dir(client)}},
	} {
		plan, err := delivery.UpdateManaged(context.Background(), release, testkit.Version, state, targets, true)
		if err != nil || len(plan.Targets) != 1 || plan.Targets[0].Path != client {
			t.Fatalf("resolved plan: %+v %v", plan, err)
		}
		if recovery == "" {
			recovery = plan.Targets[0].Recovery
		} else if plan.Targets[0].Recovery != recovery {
			t.Fatalf("selector changed transaction identity: %+v", plan)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	interrupted, err := delivery.UpdateManaged(cancelAddonUpdateAfterMove{ctx, recovery, cancel}, release, testkit.Version, state, []delivery.UpdateTarget{{Component: "addon", Path: filepath.Dir(client)}}, false)
	if !errors.Is(err, context.Canceled) || interrupted.Complete {
		t.Fatalf("interruption: %+v %v", interrupted, err)
	}
	assertDefaultAddonUpdateResumes(t, release, client, state, recovery)
}

func TestAddonUpdateResumesLegacyGameRootRecovery(t *testing.T) {
	for _, checkpoint := range []string{"old-moved", "cleanup", "archive-removed"} {
		t.Run(checkpoint, func(t *testing.T) {
			release, client, state := addonUpdateFixture(t)
			// The old implementation hashed the unresolved root, but remembered
			// the resolved client. Preserve that already-published transaction.
			key := strings.ToLower("addon:" + filepath.Dir(client))
			digest := sha256.Sum256([]byte(key + ":" + testkit.Version + ":" + strings.Repeat("b", 40)))
			recovery := filepath.Join(client, "Interface", ".lycheedev-updates", fmt.Sprintf("replace-%x", digest[:16]))
			if err := os.MkdirAll(filepath.Dir(recovery), 0700); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if checkpoint == "old-moved" {
				ctx = cancelAddonUpdateAfterMove{ctx, recovery, cancel}
			}
			_, err := delivery.UpgradeAddon(ctx, release, client, recovery, testkit.Version, false)
			if checkpoint == "old-moved" && !errors.Is(err, context.Canceled) || checkpoint != "old-moved" && err != nil {
				t.Fatalf("checkpoint: %v", err)
			}
			if checkpoint != "old-moved" {
				raw, err := os.ReadFile(filepath.Join(recovery, "replacement.json"))
				if err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(recovery+".cleanup.json", raw, 0600); err != nil {
					t.Fatal(err)
				}
				if checkpoint == "archive-removed" {
					if err := os.RemoveAll(recovery); err != nil {
						t.Fatal(err)
					}
				}
			}
			raw, err := json.Marshal([]delivery.UpdateTarget{{Component: "addon", Path: client}})
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(state, "targets.json"), raw, 0600); err != nil {
				t.Fatal(err)
			}
			assertDefaultAddonUpdateResumes(t, release, client, state, recovery)
		})
	}
}

func assertDefaultAddonUpdateResumes(t *testing.T, release, client, state, recovery string) {
	t.Helper()
	targets, err := delivery.DiscoverUpdateTargets(context.Background(), state)
	if err != nil || len(targets) != 1 || targets[0].Path != client {
		t.Fatalf("default discovery: %+v %v", targets, err)
	}
	plan, err := delivery.UpdateManaged(context.Background(), release, testkit.Version, state, targets, true)
	if err != nil || len(plan.Targets) != 1 || plan.Targets[0].Recovery != recovery {
		t.Fatalf("original recovery lost: %+v %v", plan, err)
	}
	result, err := delivery.UpdateManaged(context.Background(), release, testkit.Version, state, targets, false)
	if err != nil || !result.Complete {
		t.Fatalf("default recovery: %+v %v", result, err)
	}
	for _, path := range []string{recovery, recovery + ".cleanup.json"} {
		if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("original transaction left behind: %s: %v", path, err)
		}
	}
	installed, err := delivery.InspectInstallation(context.Background(), delivery.AddonDirectory(client), "addon")
	if err != nil || installed.State != "managed" || installed.Receipt.Commit != strings.Repeat("b", 40) {
		t.Fatalf("final installation: %+v %v", installed, err)
	}
	result, err = delivery.UpdateManaged(context.Background(), release, testkit.Version, state, []delivery.UpdateTarget{{Component: "addon", Path: filepath.Dir(client)}}, false)
	if err != nil || !result.Complete {
		t.Fatalf("original root retry: %+v %v", result, err)
	}
}
