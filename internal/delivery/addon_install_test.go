package delivery_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func seedLegacySlotPool(t *testing.T, parent, version string, pending bool) {
	t.Helper()
	pool := delivery.SlotPool{Schema: "lycheedev.slots.v1", Version: version, State: "ready", Files: make([]delivery.SlotFile, 64)}
	for index := 1; index <= len(pool.Files); index++ {
		dir := delivery.SlotDirectory(parent, index)
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
		name := fmt.Sprintf("Lychee Dev Slot %02d", index)
		files := map[string][]byte{
			name + ".toc": []byte(fmt.Sprintf("## Interface: 120100, 50504, 38002, 16001\n## Title: Lychee Dev input slot %02d\n## Version: %s\n## LoadOnDemand: 1\n## X-Lychee-Slot: %d\n## X-Lychee-Transport: memory-slot-v1\nPayload.lua\nLoader.lua\n", index, version, index)),
			"Loader.lua":  []byte(fmt.Sprintf("local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive(%d, envelope) end\n", index)),
			"Payload.lua": []byte("LycheeDevSlotEnvelope = nil\n"),
		}
		for fileName, content := range files {
			if err := os.WriteFile(filepath.Join(dir, fileName), content, 0600); err != nil {
				t.Fatal(err)
			}
		}
		digest := sha256.Sum256(files["Payload.lua"])
		pool.Files[index-1].PayloadHash = hex.EncodeToString(digest[:])
	}
	if pending {
		pool.Files[0].Nonce = strings.Repeat("a", 32)
		pool.Files[0].PendingHash = strings.Repeat("b", 64)
	}
	marker, err := json.MarshalIndent(pool, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(parent, ".lycheedev-slots.json"), marker, 0600); err != nil {
		t.Fatal(err)
	}
}

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

func TestAddonInstallDoesNotCreateLoDSlotPool(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	release := testkit.Release(t, "slot-runtime") // A historical receipt still naming SlotRuntime is not a reason to generate slots.
	if _, err := delivery.InstallAddon(ctx, release, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Join(client, "Interface", "AddOns")
	if _, err := os.Stat(filepath.Join(parent, ".lycheedev-slots.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("install wrote the legacy slot marker: %v", err)
	}
	if entries, err := os.ReadDir(parent); err != nil {
		t.Fatal(err)
	} else {
		for _, entry := range entries {
			if strings.HasPrefix(entry.Name(), "Lychee Dev Slot ") {
				t.Fatalf("install created legacy slot directory %s", entry.Name())
			}
		}
	}
}

func TestAddonInstallRequiresUpgradeWhenManagedLegacyPoolExists(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	legacyRelease := testkit.Release(t, "slot-runtime")
	if _, err := delivery.InstallAddon(ctx, legacyRelease, client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Join(client, "Interface", "AddOns")
	seedLegacySlotPool(t, parent, testkit.Version, false)
	markerPath := filepath.Join(parent, ".lycheedev-slots.json")
	before, err := os.ReadFile(markerPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = delivery.InstallAddon(ctx, legacyRelease, client, testkit.Version); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("repeat install retained a managed legacy pool: %v", err)
	}
	after, err := os.ReadFile(markerPath)
	if err != nil || string(after) != string(before) {
		t.Fatalf("legacy receipt changed: %v", err)
	}
}

func TestAddonInstallBlocksAndPreservesUnmanagedSlotDirectory(t *testing.T) {
	client := testkit.Client(t, "flavor")
	parent := filepath.Join(client, "Interface", "AddOns")
	unknown := filepath.Join(parent, "Lychee Dev Slot 07")
	if err := os.Mkdir(unknown, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(unknown, "user.lua")
	if err := os.WriteFile(sentinel, []byte("user data"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := delivery.InstallAddon(context.Background(), testkit.Release(t, ""), client, testkit.Version); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("unmanaged slot directory did not block install: %v", err)
	}
	if got, err := os.ReadFile(sentinel); err != nil || string(got) != "user data" {
		t.Fatalf("unmanaged directory changed: %q %v", got, err)
	}
	if _, err := os.Stat(delivery.AddonDirectory(client)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("main addon unexpectedly installed: %v", err)
	}
}

func TestAddonUpgradeArchivesIdleManagedSlots(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Join(client, "Interface", "AddOns")
	seedLegacySlotPool(t, parent, testkit.Version, false)
	archive := filepath.Join(t.TempDir(), "upgrade")
	if _, err := delivery.UpgradeAddon(ctx, testkit.Release(t, ""), client, archive, testkit.Version, false); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(parent, ".lycheedev-slots.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("legacy marker remains live: %v", err)
	}
	if _, err := os.Stat(delivery.SlotDirectory(parent, 1)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("legacy slot remains live: %v", err)
	}
	if _, err := os.Stat(filepath.Join(archive+".slots", ".lycheedev-slots.json")); err != nil {
		t.Fatalf("managed legacy receipt was not archived: %v", err)
	}
	status, err := delivery.InspectAddonDeployment(ctx, client)
	if err != nil || status.Installation.State != "managed" {
		t.Fatalf("upgrade health: %+v %v", status, err)
	}
}

func TestAddonUpgradeBlocksPendingOrModifiedLegacySlots(t *testing.T) {
	for _, mode := range []string{"pending", "modified"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			client := testkit.Client(t, "flavor")
			if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
				t.Fatal(err)
			}
			parent := filepath.Join(client, "Interface", "AddOns")
			seedLegacySlotPool(t, parent, testkit.Version, mode == "pending")
			var changedPath string
			if mode == "modified" {
				changedPath = filepath.Join(delivery.SlotDirectory(parent, 1), "Loader.lua")
				if err := os.WriteFile(changedPath, []byte("user change"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			mainPath := filepath.Join(delivery.AddonDirectory(client), "Core", "Start.lua")
			before, err := os.ReadFile(mainPath)
			if err != nil {
				t.Fatal(err)
			}
			archive := filepath.Join(t.TempDir(), "upgrade")
			if _, err = delivery.UpgradeAddon(ctx, testkit.Release(t, ""), client, archive, testkit.Version, false); !errors.Is(err, delivery.ErrConflict) {
				t.Fatalf("unsafe legacy pool admitted: %v", err)
			}
			after, err := os.ReadFile(mainPath)
			if err != nil || string(after) != string(before) {
				t.Fatalf("main addon changed: %q %v", after, err)
			}
			if _, err = os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("upgrade archive created before rejection: %v", err)
			}
			if mode == "modified" {
				if got, readErr := os.ReadFile(changedPath); readErr != nil || string(got) != "user change" {
					t.Fatalf("user edit changed: %q %v", got, readErr)
				}
			}
		})
	}
}

func TestRemoveAddonArchivesManagedInstallationWithoutSlots(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, ""), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	result, err := delivery.RemoveAddon(ctx, client, archive)
	if err != nil || result.State != "archived" || result.Archive != archive || result.SlotArchive != "" {
		t.Fatalf("%+v %v", result, err)
	}
}

func TestRemoveAddonWithSlotsReportsModifiedMainAsConflict(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	main := delivery.AddonDirectory(client)
	path := filepath.Join(main, "Core", "Runtime.lua")
	if err := os.WriteFile(path, []byte("-- user edit\n"), 0600); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	if _, err := delivery.RemoveAddon(ctx, client, archive); !errors.Is(err, delivery.ErrConflict) {
		t.Fatalf("modified addon must remain an installation conflict: %v", err)
	}
	if got, err := os.ReadFile(path); err != nil || string(got) != "-- user edit\n" {
		t.Fatalf("user edit changed: %q %v", got, err)
	}
	for _, target := range []string{archive, archive + ".slots", archive + ".removal.json"} {
		if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("refused removal created %s: %v", target, err)
		}
	}
}

func TestRemoveAddonRefusesUnownedSlotDirectoryBeforeMovingAnything(t *testing.T) {
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	parent := filepath.Dir(delivery.AddonDirectory(client))
	unmanaged := filepath.Join(parent, "Lychee Dev Slot 64")
	if err := os.Mkdir(unmanaged, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(unmanaged, "Loader.lua"), []byte("external edit"), 0600); err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	if _, err := delivery.RemoveAddon(ctx, client, archive); err == nil {
		t.Fatal("modified slot removed")
	}
	if _, err := os.Stat(delivery.AddonDirectory(client)); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(filepath.Join(unmanaged, "Loader.lua")); err != nil || string(got) != "external edit" {
		t.Fatalf("unmanaged slot directory changed: %q %v", got, err)
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
