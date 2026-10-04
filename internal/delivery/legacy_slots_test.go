package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func legacySlotPoolFixture(t *testing.T) (string, SlotPool) {
	t.Helper()
	parent := t.TempDir()
	const payload = "LycheeDevSlotEnvelope = nil\n"
	pool := SlotPool{Schema: legacySlotPoolSchema, Version: "3.0.0", State: "ready", Files: make([]SlotFile, 64)}
	for i := 1; i <= len(pool.Files); i++ {
		dir := SlotDirectory(parent, i)
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
		for name, data := range slotStaticForSchema(i, pool.Version, pool.Schema) {
			if err := os.WriteFile(filepath.Join(dir, name), data, 0600); err != nil {
				t.Fatal(err)
			}
		}
		if err := os.WriteFile(filepath.Join(dir, "Payload.lua"), []byte(payload), 0600); err != nil {
			t.Fatal(err)
		}
		pool.Files[i-1].PayloadHash = slotDigest([]byte(payload))
	}
	raw, err := json.MarshalIndent(pool, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(parent, slotMarker), raw, 0600); err != nil {
		t.Fatal(err)
	}
	return parent, pool
}

func writeLegacyPoolMarker(t *testing.T, parent string, pool SlotPool) {
	t.Helper()
	raw, err := json.MarshalIndent(pool, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(parent, slotMarker), raw, 0600); err != nil {
		t.Fatal(err)
	}
}

func TestLegacySlotMigrationArchivesOnlyVerifiedIdlePool(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	archive := filepath.ToSlash(filepath.Join(t.TempDir(), "upgrade.slots"))
	ready, err := preflightLegacySlots(context.Background(), parent, pool.Version, archive)
	if err != nil || !ready {
		t.Fatalf("preflight: ready=%v err=%v", ready, err)
	}
	if err = archiveLegacySlots(context.Background(), parent, archive, pool.Version); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(parent, slotMarker)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("legacy receipt still in install: %v", err)
	}
	if _, err = os.Stat(SlotDirectory(parent, 1)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("legacy slot still in install: %v", err)
	}
	archived, err := readSlotPool(archive)
	if err != nil || archived.Schema != legacySlotPoolSchema || len(archived.Files) != 64 {
		t.Fatalf("archived receipt: %+v %v", archived, err)
	}
	if _, err = inspectSlotPool(context.Background(), archive, archived, pool.Version); err != nil {
		t.Fatalf("archived bytes failed verification: %v", err)
	}
	if err = archiveLegacySlots(context.Background(), parent, archive, pool.Version); err != nil {
		t.Fatalf("completed migration retry: %v", err)
	}
}

func TestLegacySlotMigrationBlocksPendingModifiedAndUnknownContent(t *testing.T) {
	t.Run("pending", func(t *testing.T) {
		parent, pool := legacySlotPoolFixture(t)
		pool.Files[0].Nonce = strings.Repeat("a", 32)
		pool.Files[0].PendingHash = strings.Repeat("b", 64)
		writeLegacyPoolMarker(t, parent, pool)
		archive := filepath.Join(t.TempDir(), "upgrade.slots")
		if ready, err := preflightLegacySlots(context.Background(), parent, pool.Version, archive); !errors.Is(err, ErrConflict) || ready {
			t.Fatalf("pending reservation admitted: ready=%v err=%v", ready, err)
		}
		if _, err := os.Stat(SlotDirectory(parent, 1)); err != nil {
			t.Fatalf("pending slot touched: %v", err)
		}
	})
	t.Run("modified", func(t *testing.T) {
		parent, pool := legacySlotPoolFixture(t)
		path := filepath.Join(SlotDirectory(parent, 1), "Loader.lua")
		if err := os.WriteFile(path, []byte("user change"), 0600); err != nil {
			t.Fatal(err)
		}
		if ready, err := preflightLegacySlots(context.Background(), parent, pool.Version, filepath.Join(t.TempDir(), "upgrade.slots")); !errors.Is(err, ErrConflict) || ready {
			t.Fatalf("modified pool admitted: ready=%v err=%v", ready, err)
		}
		if got, err := os.ReadFile(path); err != nil || string(got) != "user change" {
			t.Fatalf("modified file changed: %q %v", got, err)
		}
	})
	t.Run("unknown", func(t *testing.T) {
		parent := t.TempDir()
		unknown := filepath.Join(parent, "Lychee Dev Slot 01")
		if err := os.Mkdir(unknown, 0700); err != nil {
			t.Fatal(err)
		}
		if ready, err := preflightLegacySlots(context.Background(), parent, "3.1.1", filepath.Join(t.TempDir(), "upgrade.slots")); !errors.Is(err, ErrConflict) || ready {
			t.Fatalf("unreceipted pool admitted: ready=%v err=%v", ready, err)
		}
		if _, err := os.Stat(unknown); err != nil {
			t.Fatalf("unknown directory touched: %v", err)
		}
	})
}

func TestLegacySlotMigrationResumesPartialDirectoryMoves(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	archive := filepath.Join(t.TempDir(), "upgrade.slots")
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
	for i := 1; i <= 20; i++ {
		if err = os.Rename(SlotDirectory(parent, i), SlotDirectory(archive, i)); err != nil {
			t.Fatal(err)
		}
	}
	if err = archiveLegacySlots(context.Background(), parent, archive, pool.Version); err != nil {
		t.Fatalf("resume partial migration: %v", err)
	}
	if _, err = os.Stat(filepath.Join(parent, slotMarker)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("source marker remains: %v", err)
	}
	if _, err = readSlotPool(archive); err != nil {
		t.Fatalf("archive receipt: %v", err)
	}
	if _, err = os.Stat(archive + ".migration.json"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("migration intent remains: %v", err)
	}
}
