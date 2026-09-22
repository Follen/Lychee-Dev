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
