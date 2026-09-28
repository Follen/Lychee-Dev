package delivery_test

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"testing"
)

func TestInstallAddonFirstInstallAndRetry(t *testing.T) {
	release := testkit.Release(t, "")
	client := testkit.Client(t, "flavor")

	first, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version)
	if err != nil {
		t.Fatal(err)
	}
	second, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version)
	if err != nil {
		t.Fatal(err)
	}
	if first.Schema != "lycheedev.installation.v1" || first.Component != "addon" || first.Version != testkit.Version || first.Commit == "" {
		t.Fatalf("unexpected receipt: %+v", first)
	}
	if !sameAddonReceipt(first, second) {
		t.Fatalf("retry changed receipt: first=%+v second=%+v", first, second)
	}
	assessment, err := delivery.InspectInstallation(context.Background(), filepath.Join(client, "Interface", "AddOns", "Lychee Dev"), "addon")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("installed addon: %+v %v", assessment, err)
	}
}

func TestSlotUpgradeResumesAfterMainPublicationAndStatusIncludesPool(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, ""), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "upgrade")
	// Model interruption after the main transaction committed, before slots.
	if _, err := delivery.UpgradeInstallation(ctx, testkit.Release(t, "slot-runtime"), delivery.AddonDirectory(client), archive, "addon", testkit.Version); err != nil {
		t.Fatal(err)
	}
	status, err := delivery.InspectAddonDeployment(ctx, client)
	if err != nil || status.Installation.State != "managed" || status.Slots == nil || status.Slots.State != "incomplete" {
		t.Fatalf("%+v %v", status, err)
	}
	if _, err = delivery.UpgradeAddon(ctx, "", client, archive, testkit.Version, true); err != nil {
		t.Fatal(err)
	}
	status, err = delivery.InspectAddonDeployment(ctx, client)
	if err != nil || status.Slots.State != "managed" || status.Slots.Count != 64 {
		t.Fatalf("%+v %v", status, err)
	}
	path := filepath.Join(client, "Interface", "AddOns", "Lychee Dev Slot 64", "Loader.lua")
	if err = os.WriteFile(path, []byte("external drift"), 0600); err != nil {
		t.Fatal(err)
	}
	status, err = delivery.InspectAddonDeployment(ctx, client)
	if err != nil || status.Slots.State != "incomplete" {
		t.Fatalf("slot drift was hidden: %+v %v", status, err)
	}
}

func TestRemoveAddonIncludesSlotsAndResumesPartialMoves(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Dir(delivery.AddonDirectory(client))
	archive := filepath.Join(t.TempDir(), "removed")
	result, err := delivery.RemoveAddon(ctx, client, archive)
	if err != nil || result.State != "archived" || result.SlotArchive != archive+".slots" {
		t.Fatalf("%+v %v", result, err)
	}
	for i := 1; i <= 64; i++ {
		if _, err := os.Stat(delivery.SlotDirectory(parent, i)); !errors.Is(err, os.ErrNotExist) {
			t.Fatal("slot remains", i, err)
		}
	}
	// Recreate an interruption after the first 32 slot moves. The intent remains
	// durable; retry must validate both sides and finish the exact same removal.
	for i := 33; i <= 64; i++ {
		if err := os.Rename(delivery.SlotDirectory(result.SlotArchive, i), delivery.SlotDirectory(parent, i)); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Rename(filepath.Join(result.SlotArchive, ".lycheedev-slots.json"), filepath.Join(parent, ".lycheedev-slots.json")); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(archive, delivery.AddonDirectory(client)); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		resumed, err := delivery.RemoveAddon(ctx, client, archive)
		if err != nil || resumed.State != "archived" || resumed.SlotArchive != result.SlotArchive {
			t.Fatalf("retry %d: %+v %v", i, resumed, err)
		}
	}
	if _, err := delivery.InspectSlots(ctx, result.SlotArchive, testkit.Version); err != nil {
		t.Fatal("archived pool failed integrity", err)
	}
}

func TestRemoveAddonRefusesModifiedSlotBeforeMovingAnything(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Dir(delivery.AddonDirectory(client))
	if err := os.WriteFile(filepath.Join(delivery.SlotDirectory(parent, 64), "Loader.lua"), []byte("external edit"), 0600); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	if _, err := delivery.RemoveAddon(ctx, client, archive); err == nil {
		t.Fatal("modified slot removed")
	}
	if _, err := os.Stat(delivery.AddonDirectory(client)); err != nil {
		t.Fatal(err)
	}
	for i := 1; i <= 64; i++ {
		if _, err := os.Stat(delivery.SlotDirectory(parent, i)); err != nil {
			t.Fatal(i, err)
		}
	}
	if _, err := os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("archive unexpectedly created", err)
	}
}

func TestInstallAddonResolvesActiveCatalogWhenClientFilesAreAbsent(t *testing.T) {
	release := testkit.Release(t, "")
	client := testkit.Client(t, "catalog")

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
}

func TestInstallAddonPreservesUnmanagedDirectory(t *testing.T) {
	release := testkit.Release(t, "")
	client := testkit.Client(t, "flavor")
	target := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")
	sentinel := filepath.Join(target, "user-file.txt")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(sentinel, []byte("user content"), 0600); err != nil {
		t.Fatal(err)
	}

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("unmanaged directory was adopted: %v", err)
	}
	content, err := os.ReadFile(sentinel)
	if err != nil || string(content) != "user content" {
		t.Fatalf("unmanaged content changed: %q %v", content, err)
	}
}

func TestInstallAddonRefusesModifiedManagedInstallation(t *testing.T) {
	release := testkit.Release(t, "")
	client := testkit.Client(t, "flavor")
	target := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")
	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	edited := []byte("local edited = true\n")
	start := filepath.Join(target, "Core", "Start.lua")
	if err := os.WriteFile(start, edited, 0600); err != nil {
		t.Fatal(err)
	}

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("modified managed installation was replaced: %v", err)
	}
	content, err := os.ReadFile(start)
	if err != nil || string(content) != string(edited) {
		t.Fatalf("edited Start.lua changed: %q %v", content, err)
	}
}

func TestInstallAddonRefusesWrongTOCBeforeInstalling(t *testing.T) {
	release := testkit.Release(t, "wrong-interface")
	client := testkit.Client(t, "flavor")
	target := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err == nil {
		t.Fatal("accepted release with a wrong TOC")
	}
	if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("created addon after rejecting release: %v", err)
	}
}

func TestInstallAddonRefusesUnsupportedProduct(t *testing.T) {
	release := testkit.Release(t, "")
	client := testkit.Client(t, "unsupported")

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err == nil {
		t.Fatal("accepted unsupported client product")
	}
	if _, err := os.Stat(filepath.Join(client, "Interface", "AddOns", "Lychee Dev")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("created addon for unsupported product: %v", err)
	}
}

func TestInstallAddonRequiresOrdinaryAddonParents(t *testing.T) {
	release := testkit.Release(t, "")
	root := t.TempDir()
	client := filepath.Join(root, "_retail_")
	if err := os.MkdirAll(filepath.Join(client, "Interface"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(client, ".flavor.info"), []byte("Product Flavor!STRING:0\nwow\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(client, "version.txt"), []byte("12.1.0.69875\n"), 0600); err != nil {
		t.Fatal(err)
	}

	if _, err := delivery.InstallAddon(context.Background(), release, client, testkit.Version); err == nil {
		t.Fatal("accepted client without Interface/AddOns")
	}
}
