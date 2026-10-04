//go:build windows

package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestLegacySlotMigrationResumesForwardSlashEmptyArchive(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	// The CLI preserves the original archive spelling in its durable intent.
	archive := filepath.ToSlash(filepath.Join(t.TempDir(), "upgrade.slots"))
	marker, err := readSlotFile(filepath.Join(parent, slotMarker), slotPoolManifestLimit)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(marker)
	intent := legacySlotMigration{Schema: legacySlotMigrationSchema, Parent: parent, Archive: archive, Version: pool.Version, MarkerSHA256: hex.EncodeToString(digest[:]), Pool: pool}
	raw, err := json.Marshal(intent)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(archive+".migration.json", raw, 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Mkdir(archive, 0700); err != nil {
		t.Fatal(err)
	}
	if err = archiveLegacySlots(context.Background(), parent, archive, pool.Version); err != nil {
		t.Fatalf("resume unchanged intent after archive creation: %v", err)
	}
	archived, err := readSlotPool(archive)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = inspectSlotPool(context.Background(), archive, archived, pool.Version); err != nil {
		t.Fatalf("archived receipt/bytes: %v", err)
	}
	if _, err = os.Stat(filepath.Join(parent, slotMarker)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("source marker remains: %v", err)
	}
	if _, err = os.Stat(archive + ".migration.json"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("migration intent remains: %v", err)
	}
}

func TestPlainSlotDirectoryRejectsJunctions(t *testing.T) {
	for _, ancestor := range []bool{false, true} {
		t.Run(map[bool]string{false: "leaf", true: "ancestor"}[ancestor], func(t *testing.T) {
			root := t.TempDir()
			target := filepath.Join(root, "target")
			if err := os.MkdirAll(filepath.Join(target, "slot"), 0700); err != nil {
				t.Fatal(err)
			}
			link := filepath.Join(root, "junction")
			// Directory junctions do not require Windows symlink privileges.
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, "cmd", "/d", "/c", "mklink", "/J", link, target)
			if output, err := cmd.CombinedOutput(); err != nil {
				t.Fatalf("create junction fixture: %v\n%s", err, output)
			}
			path := link
			if ancestor {
				path = filepath.Join(link, "slot")
			}
			if _, err := os.Stat(path); err != nil {
				t.Fatalf("junction fixture must resolve to an existing directory: %v", err)
			}
			if err := plainSlotDirectory(filepath.ToSlash(path)); err == nil {
				t.Fatalf("actual redirected directory accepted: %v", err)
			}
		})
	}
}
