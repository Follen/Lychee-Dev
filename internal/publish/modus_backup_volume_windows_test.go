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

func TestModusBackupVolumeContainment(t *testing.T) {
	for _, tc := range []struct {
		root, output string
		outside      bool
	}{
		{`C:\Games\WoW`, `D:\Backups\WoW`, true},
		{`C:\Games\WoW`, `C:\Backups\WoW`, true},
		{`C:\Games\WoW`, `C:\Games\WoW-backup`, true},
		{`C:\Games\WoW`, `c:\games\wow`, false},
		{`C:\Games\WoW`, `C:\Games\WoW\backup`, false},
	} {
		if got := backupPathOutside(tc.root, tc.output); got != tc.outside {
			t.Errorf("%q -> %q: outside=%t, want %t", tc.root, tc.output, got, tc.outside)
		}
	}
}

func TestModusBackupOutputContainment(t *testing.T) {
	root, _, input := modusBackupTestFiles(t)
	for _, tc := range []struct {
		output string
		valid  bool
	}{
		{root, false},
		{filepath.Join(root, "nested", "backup"), false},
		{root + "-backup", true},
	} {
		input["output_dir"] = tc.output
		raw, _ := json.Marshal(input)
		result, err := RunModus(context.Background(), "backup generate", Request{Input: raw, Plan: true})
		if (err == nil && result.Complete) != tc.valid {
			t.Errorf("output=%q complete=%t: %v", tc.output, result.Complete, err)
		}
	}
	t.Run("resolved_link_into_game", func(t *testing.T) {
		alias := filepath.Join(t.TempDir(), "game-alias")
		// Directory junctions do not require the symlink privilege on Windows.
		if output, err := exec.Command("cmd.exe", "/c", "mklink", "/J", alias, root).CombinedOutput(); err != nil {
			t.Fatalf("create fixture junction: %v: %s", err, output)
		}
		input["output_dir"] = filepath.Join(alias, "backup")
		raw, _ := json.Marshal(input)
		result, err := RunModus(context.Background(), "backup generate", Request{Input: raw, Plan: true})
		if err == nil || result.Complete {
			t.Fatal("resolved output inside game was accepted")
		}
	})
}

func TestModusBackupGenerateAcrossVolumes(t *testing.T) {
	parent := os.Getenv("LYCHEEDEV_TEST_BACKUP_VOLUME")
	if parent == "" {
		t.Skip("set LYCHEEDEV_TEST_BACKUP_VOLUME to a writable directory on a second volume")
	}
	root, _, input := modusBackupTestFiles(t)
	parent, err := filepath.Abs(parent)
	if err != nil {
		t.Fatal(err)
	}
	if strings.EqualFold(filepath.VolumeName(root), filepath.VolumeName(parent)) {
		t.Fatal("backup fixture requires distinct source and destination volumes")
	}
	base, err := os.MkdirTemp(parent, "modus-backup-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(base) })
	output := filepath.Join(base, "backup")
	input["output_dir"] = output
	raw, err := json.Marshal(input)
	if err != nil {
		t.Fatal(err)
	}
	for _, plan := range []bool{true, false} {
		result, err := RunModus(context.Background(), "backup generate", Request{Input: raw, Plan: plan})
		if err != nil || !result.Complete {
			t.Fatalf("plan=%t complete=%t: %v", plan, result.Complete, err)
		}
		if plan {
			if _, err := os.Stat(output); !os.IsNotExist(err) {
				t.Fatal("plan created output")
			}
		}
	}
	if _, _, err := loadModusBackupManifest(context.Background(), filepath.Join(output, "manifest.v2.json"), 0); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(output, "Interface", "AddOns", "Selected", "a.lua"))
	if err != nil || string(data) != "selected" {
		t.Fatalf("cross-volume backup content: %q, %v", data, err)
	}
}
