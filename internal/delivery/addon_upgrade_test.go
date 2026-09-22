package delivery_test

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestUpgradeAddonResumeAndRemovePreserveOwnedState(t *testing.T) {
	ctx := context.Background()
	release := testkit.Release(t, "")
	client := testkit.Client(t, "flavor")
	target := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")
	upgradeArchive := filepath.Join(t.TempDir(), "upgrade")
	removeArchive := filepath.Join(t.TempDir(), "removed")
	savedVariables := filepath.Join(client, "WTF", "Account", "Test", "SavedVariables", "sentinel.lua")
	if err := os.MkdirAll(filepath.Dir(savedVariables), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(savedVariables, []byte("sentinel = true\n"), 0600); err != nil {
		t.Fatal(err)
	}

	if _, err := delivery.InstallAddon(ctx, release, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	if err := addonUpgradeCommit(t, release, strings.Repeat("e", 40)); err != nil {
		t.Fatal(err)
	}

	upgraded, err := delivery.UpgradeAddon(ctx, release, client, upgradeArchive, testkit.Version, false)
	if err != nil || upgraded.Receipt.Commit != strings.Repeat("e", 40) {
		t.Fatalf("upgrade: %+v %v", upgraded, err)
	}
	previous, err := delivery.InspectInstallation(ctx, filepath.Join(upgradeArchive, "previous"), "addon")
	if err != nil || previous.State != "managed" || previous.Receipt.Commit != strings.Repeat("c", 40) {
		t.Fatalf("previous installation: %+v %v", previous, err)
	}

	if err := os.Remove(filepath.Join(release, "release.json")); err != nil {
		t.Fatal(err)
	}
	resumed, err := delivery.UpgradeAddon(ctx, filepath.Join(t.TempDir(), "missing-release"), client, upgradeArchive, testkit.Version, true)
	if err != nil || resumed.Receipt.Commit != strings.Repeat("e", 40) {
		t.Fatalf("resume without source: %+v %v", resumed, err)
	}

	removed, err := delivery.RemoveAddon(ctx, client, removeArchive)
	if err != nil || removed.State != "archived" {
		t.Fatalf("remove: %+v %v", removed, err)
	}
	if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("target still exists after remove: %v", err)
	}
	archived, err := delivery.InspectInstallation(ctx, removeArchive, "addon")
	if err != nil || archived.State != "managed" || archived.Receipt.Commit != strings.Repeat("e", 40) {
		t.Fatalf("remove archive: %+v %v", archived, err)
	}
	content, err := os.ReadFile(savedVariables)
	if err != nil || string(content) != "sentinel = true\n" {
		t.Fatalf("saved variables changed: %q %v", content, err)
	}
}

func TestUpgradeAddonRejectsInvalidReleaseAndRemoveRejectsModifiedCurrent(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, ""), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")
	start := filepath.Join(target, "Core", "Start.lua")
	original, err := os.ReadFile(start)
	if err != nil {
		t.Fatal(err)
	}

	invalidRelease := testkit.Release(t, "wrong-interface")
	if _, err := delivery.UpgradeAddon(ctx, invalidRelease, client, filepath.Join(t.TempDir(), "upgrade"), testkit.Version, false); err == nil {
		t.Fatal("accepted release with an invalid TOC")
	}
	current, err := delivery.InspectInstallation(ctx, target, "addon")
	if err != nil || current.State != "managed" || current.Receipt.Commit != strings.Repeat("c", 40) {
		t.Fatalf("invalid upgrade changed current installation: %+v %v", current, err)
	}
	unchanged, err := os.ReadFile(start)
	if err != nil || string(unchanged) != string(original) {
		t.Fatalf("invalid upgrade changed addon bytes: %q %v", unchanged, err)
	}

	modified := []byte("local edited = true\n")
	if err := os.WriteFile(start, modified, 0600); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	if _, err := delivery.RemoveAddon(ctx, client, archive); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("removed modified installation: %v", err)
	}
	unchanged, err = os.ReadFile(start)
	if err != nil || string(unchanged) != string(modified) {
		t.Fatalf("modified addon changed after refused removal: %q %v", unchanged, err)
	}
	if _, err := os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("created removal archive after refusal: %v", err)
	}
}
