//go:build windows

package publish

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func ddBackupVolumeFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	addon := filepath.Join(root, "Interface", "AddOns", "Selected")
	if err := os.MkdirAll(addon, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(addon, "Main.lua"), []byte("selected"), 0600); err != nil {
		t.Fatal(err)
	}
	return root
}
func ddGenerateVolumeFixture(t *testing.T, root, output string) (Result, error) {
	t.Helper()
	raw, err := json.Marshal(map[string]any{"root": root, "output": output, "game_type": 1, "unknown_addons": []string{"Selected"}})
	if err != nil {
		t.Fatal(err)
	}
	return RunDD(context.Background(), "backup generate", Request{Input: raw})
}
func TestDDBackupGenerateAcrossVolumes(t *testing.T) {
	parent := os.Getenv("LYCHEEDEV_TEST_BACKUP_VOLUME")
	if parent == "" {
		t.Skip("set LYCHEEDEV_TEST_BACKUP_VOLUME to a writable directory on a second volume")
	}
	root := ddBackupVolumeFixture(t)
	parent, err := filepath.Abs(parent)
	if err != nil {
		t.Fatal(err)
	}
	if strings.EqualFold(filepath.VolumeName(root), filepath.VolumeName(parent)) {
		t.Fatal("backup fixture requires distinct volumes")
	}
	base, err := os.MkdirTemp(parent, "dd-backup-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(base) })
	output := filepath.Join(base, "backup")
	result, err := ddGenerateVolumeFixture(t, root, output)
	if err != nil || !result.Complete {
		t.Fatalf("cross-volume generation complete=%t: %v", result.Complete, err)
	}
	bundle, err := ddLoadBundle(filepath.Join(output, "backup-bundle.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer bundle.close()
	if len(bundle.bundle.Artifacts) != 1 || len(bundle.bundle.Artifacts[0].Members) != 1 || bundle.bundle.Artifacts[0].Members[0] != "Selected/Main.lua" {
		t.Fatalf("wrong selected content: %+v", bundle.bundle.Artifacts)
	}
}
func TestDDBackupGenerateRejectsJunctionOutput(t *testing.T) {
	root := ddBackupVolumeFixture(t)
	alias := filepath.Join(t.TempDir(), "game-alias")
	if out, err := exec.Command("cmd.exe", "/c", "mklink", "/J", alias, root).CombinedOutput(); err != nil {
		t.Fatalf("create fixture junction: %v: %s", err, out)
	}
	result, err := ddGenerateVolumeFixture(t, root, filepath.Join(alias, "backup"))
	if err == nil || result.Complete {
		t.Fatal("output junction into game accepted")
	}
	if _, err := os.Stat(filepath.Join(root, "backup")); !os.IsNotExist(err) {
		t.Fatalf("rejected generation created output: %v", err)
	}
}
