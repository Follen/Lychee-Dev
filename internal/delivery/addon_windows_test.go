package delivery_test

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestAddonMutationsRejectRedirectedDiscoveryDirectory(t *testing.T) {
	client := testkit.Client(t, "flavor")
	link := filepath.Join(client, "Interface", "AddOns")
	if err := os.Remove(link); err != nil {
		t.Fatal(err)
	}
	outside := t.TempDir()
	command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
	command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+outside)
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("junction: %v %s", err, output)
	}
	defer func() {
		if err := os.Remove(link); err != nil {
			t.Error(err)
		}
	}()
	ctx := context.Background()
	archive := filepath.Join(t.TempDir(), "archive")
	_, installErr := delivery.InstallAddon(ctx, "unused-release", client, testkit.Version)
	_, upgradeErr := delivery.UpgradeAddon(ctx, "unused-release", client, archive, testkit.Version, false)
	_, resumeErr := delivery.UpgradeAddon(ctx, "", client, archive, testkit.Version, true)
	_, removeErr := delivery.RemoveAddon(ctx, client, archive)
	for _, err := range []error{installErr, upgradeErr, resumeErr, removeErr} {
		if !errors.Is(err, delivery.ErrInstallation) {
			t.Fatalf("redirect not rejected: %v", err)
		}
	}
	entries, err := os.ReadDir(outside)
	if err != nil || len(entries) != 0 {
		t.Fatalf("redirect target changed: %v %v", entries, err)
	}
}
