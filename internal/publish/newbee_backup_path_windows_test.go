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

func TestNewBeeBackupPackRejectsOutputJunctionIntoGame(t *testing.T) {
	dir := t.TempDir()
	game, alias := filepath.Join(dir, "game"), filepath.Join(dir, "outside")
	plugin := filepath.Join(game, "Interface", "AddOns", "Selected")
	inside := filepath.Join(game, "backups")
	for _, path := range []string{plugin, inside} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(plugin, "Selected.toc"), []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	if output, err := exec.Command("cmd", "/c", "mklink", "/J", alias, inside).CombinedOutput(); err != nil {
		t.Fatalf("junction: %s %v", output, err)
	}
	raw, _ := json.Marshal(map[string]any{"game_dir": game, "output_dir": filepath.Join(alias, "new-bundle"), "game_version_id": 1, "unknown_plugins": []string{"Selected"}})
	result, err := RunNewBee(context.Background(), "creator backup pack", Request{RequestID: "local-pack", Input: raw})
	if err == nil || result.Complete {
		t.Fatalf("output junction wrote inside game: %+v %v", result, err)
	}
	if _, err := os.Stat(filepath.Join(inside, "new-bundle")); !os.IsNotExist(err) {
		t.Fatalf("rejected output created files: %v", err)
	}
}

func TestNewBeeBackupPackAcrossVolumes(t *testing.T) {
	parent := os.Getenv("LYCHEEDEV_TEST_BACKUP_VOLUME")
	if parent == "" {
		t.Skip("set LYCHEEDEV_TEST_BACKUP_VOLUME to a writable directory on a second volume")
	}
	game := t.TempDir()
	parent, err := filepath.Abs(parent)
	if err != nil {
		t.Fatal(err)
	}
	if strings.EqualFold(filepath.VolumeName(game), filepath.VolumeName(parent)) {
		t.Fatal("backup fixture requires a second volume")
	}
	output, err := os.MkdirTemp(parent, "newbee-volume-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.RemoveAll(output); err != nil {
			t.Error(err)
		}
	})
	plugin := filepath.Join(game, "Interface", "AddOns", "Selected")
	if err := os.MkdirAll(plugin, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(plugin, "Selected.toc"), []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(map[string]any{"game_dir": game, "output_dir": output, "game_version_id": 1, "unknown_plugins": []string{"Selected"}})
	result, err := RunNewBee(context.Background(), "creator backup pack", Request{RequestID: "cross-volume", Input: raw})
	if err != nil || !result.Complete {
		t.Fatalf("cross-volume pack: %+v %v", result, err)
	}
	if _, err := newBeeReadBundle(filepath.Join(output, "backup-bundle.json")); err != nil {
		t.Fatalf("cross-volume bundle: %v", err)
	}
}
