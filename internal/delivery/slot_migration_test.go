package delivery

import (
	"context"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func legacySlotPoolFixture(t *testing.T) (string, SlotPool) {
	t.Helper()
	parent := t.TempDir()
	pool := SlotPool{Schema: "lycheedev.slots.v1", Version: "3.0.0", State: "ready", Files: make([]SlotFile, 64)}
	for i := 1; i <= 64; i++ {
		dir := SlotDirectory(parent, i)
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
		for name, data := range slotStatic(i, pool.Version) {
			data = []byte(strings.ReplaceAll(string(data), "memory-slot-v2", "memory-slot-v1"))
			if err := os.WriteFile(filepath.Join(dir, name), data, 0600); err != nil {
				t.Fatal(err)
			}
		}
		if err := os.WriteFile(filepath.Join(dir, "Payload.lua"), []byte(inertSlot), 0600); err != nil {
			t.Fatal(err)
		}
		pool.Files[i-1].PayloadHash = slotDigest([]byte(inertSlot))
	}
	if err := saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	return parent, pool
}

func TestSlotPoolLegacyMigrationAndInspection(t *testing.T) {
	parent, legacy := legacySlotPoolFixture(t)
	ctx := context.Background()
	if pool, err := InspectSlots(ctx, parent, legacy.Version); err != nil || len(pool.Files) != 64 {
		t.Fatal("legacy inspection failed", err)
	}
	pool, err := installSlotPool(ctx, parent, legacy.Version)
	if err != nil {
		t.Fatal(err)
	}
	if pool.Schema != "lycheedev.slots.v2" || len(pool.Files) != 200 || pool.State != "ready" {
		t.Fatalf("not migrated: %+v", pool)
	}
	if _, err = InspectSlots(ctx, parent, legacy.Version); err != nil {
		t.Fatal(err)
	}
	for _, i := range []int{1, 64, 65, 100, 200} {
		data, err := os.ReadFile(filepath.Join(SlotDirectory(parent, i), fmt.Sprintf("Lychee Dev Slot %02d.toc", i)))
		if err != nil || !strings.Contains(string(data), "memory-slot-v2") {
			t.Fatal("wrong static generation", i, err)
		}
	}
	if _, err = installSlotPool(ctx, parent, legacy.Version); err != nil {
		t.Fatal("not idempotent", err)
	}
}

func TestSlotPoolLegacyMigrationPreservesUnresolvedOrForeignBytes(t *testing.T) {
	for _, mode := range []string{"unknown", "pending", "foreign_extension", "dirty_static"} {
		t.Run(mode, func(t *testing.T) {
			parent, pool := legacySlotPoolFixture(t)
			switch mode {
			case "unknown":
				pool.Files[0].Nonce = strings.Repeat("1", 32)
				pool.Files[0].Runtime = strings.Repeat("2", 32)
				pool.Files[0].Consumer = "123/456"
			case "pending":
				pool.Files[0].PendingHash = pool.Files[0].PayloadHash
			case "foreign_extension":
				if err := os.Mkdir(SlotDirectory(parent, 200), 0700); err != nil {
					t.Fatal(err)
				}
			case "dirty_static":
				if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 64), "Loader.lua"), []byte("foreign bytes"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			if err := saveSlotPool(context.Background(), parent, pool); err != nil {
				t.Fatal(err)
			}
			before, err := os.ReadFile(filepath.Join(parent, slotMarker))
			if err != nil {
				t.Fatal(err)
			}
			if _, err = installSlotPool(context.Background(), parent, pool.Version); err == nil {
				t.Fatal("unsafe migration accepted")
			}
			after, err := os.ReadFile(filepath.Join(parent, slotMarker))
			if err != nil {
				t.Fatal(err)
			}
			if string(before) != string(after) {
				t.Fatal("rejected migration changed ledger")
			}
		})
	}
}

func TestSlotPoolMigrationResumesPartialStaticAndExtension(t *testing.T) {
	for _, checkpoint := range []string{"intent", "mixed_static", "partial_extension", "all_files"} {
		t.Run(checkpoint, func(t *testing.T) {
			parent, pool := legacySlotPoolFixture(t)
			pool.Schema = slotPoolMigrationSchema
			pool.State = "upgrading"
			pool.PendingVersion = "3.0.1"
			if err := saveSlotPool(context.Background(), parent, pool); err != nil {
				t.Fatal(err)
			}
			write := func(i int, all bool) {
				dir := SlotDirectory(parent, i)
				if err := os.MkdirAll(dir, 0700); err != nil {
					t.Fatal(err)
				}
				for name, data := range slotStatic(i, "3.0.1") {
					if !all && name == "Loader.lua" {
						continue
					}
					if err := os.WriteFile(filepath.Join(dir, name), data, 0600); err != nil {
						t.Fatal(err)
					}
				}
				if all {
					if err := os.WriteFile(filepath.Join(dir, "Payload.lua"), []byte(inertSlot), 0600); err != nil {
						t.Fatal(err)
					}
				}
			}
			switch checkpoint {
			case "mixed_static":
				write(1, false)
			case "partial_extension":
				write(1, true)
				write(65, false)
			case "all_files":
				for i := 1; i <= 200; i++ {
					write(i, true)
				}
			}
			if _, err := InspectSlots(context.Background(), parent, "3.0.1"); err == nil {
				t.Fatal("incomplete migration exposed as ready")
			}
			actual, err := installSlotPool(context.Background(), parent, "3.0.1")
			if err != nil || actual.Schema != slotPoolSchema || len(actual.Files) != 200 || actual.PendingVersion != "" {
				t.Fatal("resume failed", err)
			}
			if _, err = InspectSlots(context.Background(), parent, "3.0.1"); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestSlotPoolMigrationPreflightsAllFilesBeforeResumingWrites(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	pool.Schema = slotPoolMigrationSchema
	pool.State = "upgrading"
	pool.PendingVersion = pool.Version
	if err := saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(SlotDirectory(parent, 200), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 200), "Loader.lua"), []byte("unmanaged"), 0600); err != nil {
		t.Fatal(err)
	}
	first := filepath.Join(SlotDirectory(parent, 1), "Lychee Dev Slot 01.toc")
	before, err := os.ReadFile(first)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = installSlotPool(context.Background(), parent, pool.Version); err == nil {
		t.Fatal("adopted unknown extension")
	}
	after, err := os.ReadFile(first)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) {
		t.Fatal("modified old files before completing preflight")
	}
}

func TestLegacySlotRecoveryRejectsOutOfRangeAndNewPublication(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	ctx := context.Background()
	envelope := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 65, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	for _, call := range []func() error{
		func() error { return PublishSlot(ctx, parent, pool.Version, "1/2", envelope) },
		func() error { return VerifySlotPublication(ctx, parent, pool.Version, "1/2", envelope) },
		func() error { return ConfirmSlotConsumed(ctx, parent, pool.Version, "1/2", 65, envelope.Nonce) },
		func() error { return RetireSlotProcess(ctx, parent, pool.Version, "1/2", envelope) },
		func() error {
			return RetireSlotRuntime(ctx, parent, pool.Version, "1/2", envelope, strings.Repeat("5", 32))
		},
	} {
		if err := call(); err == nil {
			t.Fatal("legacy index accepted")
		}
	}
	envelope.Index = 1
	if err := PublishSlot(ctx, parent, pool.Version, "1/2", envelope); err == nil {
		t.Fatal("new v2 publication on legacy pool")
	}
}

func TestSlotPoolExpandedManifestAndUpperSlot(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	pool, err := installSlotPool(ctx, parent, "3.0.0")
	if err != nil {
		t.Fatal(err)
	}
	for i := range pool.Files {
		slot := &pool.Files[i]
		slot.Consumer = "4294967295/18446744073709551615"
		slot.Nonce = strings.Repeat("1", 32)
		slot.Runtime = strings.Repeat("2", 32)
		slot.PendingHash = slot.PayloadHash
		slot.RetiredRuntime = strings.Repeat("3", 32)
	}
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	bytes, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil || len(bytes) <= 65536 || len(bytes) > slotPoolManifestLimit {
		t.Fatal("manifest bound fixture invalid", len(bytes), err)
	}
	if _, err = InspectSlots(ctx, parent, pool.Version); err != nil {
		t.Fatal(err)
	}
	pool.Files[199] = SlotFile{PayloadHash: slotDigest([]byte(inertSlot))}
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 200, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	if err = PublishSlot(ctx, parent, pool.Version, "1/2", e); err != nil {
		t.Fatal(err)
	}
	if err = VerifySlotPublication(ctx, parent, pool.Version, "1/2", e); err != nil {
		t.Fatal(err)
	}
	if err = RetireSlotRuntime(ctx, parent, pool.Version, "1/2", e, strings.Repeat("5", 32)); err != nil {
		t.Fatal(err)
	}
	pool, err = InspectSlots(ctx, parent, pool.Version)
	if err != nil || pool.Files[199].Consumed || pool.Files[199].RetiredRuntime == "" {
		t.Fatal("upper slot retirement failed", err)
	}
}

func TestSlotPoolMigrationIntentRejectsLegacyReaders(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	// An interruption after the intent is recorded must leave a format an
	// old CLI (which only accepts slots.v1/64) cannot mistake for its own upgrade.
	pool.Schema = slotPoolMigrationSchema
	pool.State = "upgrading"
	pool.PendingVersion = pool.Version
	if err := saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	loaded, err := readSlotPool(parent)
	if err != nil || loaded.Schema == legacySlotPoolSchema || len(loaded.Files) != 64 {
		t.Fatal("unsafe migration receipt", err)
	}
	if _, err = installSlotPool(context.Background(), parent, pool.Version); err != nil {
		t.Fatal(err)
	}
}

func TestSlotPoolMigrationResumesLegacyVersionUpgrade(t *testing.T) {
	parent, pool := legacySlotPoolFixture(t)
	pool.State = "upgrading"
	pool.PendingVersion = "3.0.1"
	if err := saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	for name, data := range slotStaticForSchema(1, "3.0.1", legacySlotPoolSchema) {
		if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 1), name), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	next, err := installSlotPool(context.Background(), parent, "3.0.1")
	if err != nil || next.Schema != slotPoolSchema || len(next.Files) != 200 {
		t.Fatal("could not resume old static version transition", err)
	}
}

func TestSlotPoolLegacyPendingPublicationResumesOriginalBytes(t *testing.T) {
	ctx := context.Background()
	parent, pool := legacySlotPoolFixture(t)
	e := bridge.SlotEnvelope{Schema: bridge.LegacySlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	payload, err := bridge.SlotPayload(e)
	if err != nil {
		t.Fatal(err)
	}
	entry := &pool.Files[0]
	entry.Nonce, entry.Runtime, entry.Consumer, entry.PendingHash = e.Nonce, e.Runtime, "123/456", slotDigest(payload)
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	if err = PublishSlot(ctx, parent, pool.Version, "123/456", e); err != nil {
		t.Fatal("old pending nonce could not resume", err)
	}
	if err = VerifySlotPublication(ctx, parent, pool.Version, "123/456", e); err != nil {
		t.Fatal(err)
	}
	actual, err := os.ReadFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"))
	if err != nil || string(actual) != string(payload) {
		t.Fatal("legacy wire changed", err)
	}
	other := e
	other.Nonce = strings.Repeat("5", 32)
	if err = PublishSlot(ctx, parent, pool.Version, "other", other); !errors.Is(err, ErrSlotReserved) {
		t.Fatal("overwrote unresolved legacy nonce", err)
	}
	if _, err = installSlotPool(ctx, parent, pool.Version); !errors.Is(err, ErrConflict) {
		t.Fatal("migrated before legacy resolution", err)
	}
	if err = ConfirmSlotConsumed(ctx, parent, pool.Version, "123/456", 1, e.Nonce); err != nil {
		t.Fatal(err)
	}
	migrated, err := installSlotPool(ctx, parent, pool.Version)
	if err != nil || migrated.Schema != slotPoolSchema {
		t.Fatal("resolved legacy could not migrate", err)
	}
}

func TestSlotPoolRejectsCrossGenerationEnvelopes(t *testing.T) {
	ctx := context.Background()
	legacy, pool := legacySlotPoolFixture(t)
	modern := t.TempDir()
	if _, err := installSlotPool(ctx, modern, pool.Version); err != nil {
		t.Fatal(err)
	}
	base := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	for _, tc := range []struct {
		name, parent, schema string
		start                int
	}{
		{"new_on_legacy", legacy, bridge.SlotSchema, 0},
		{"old_with_start", legacy, bridge.LegacySlotSchema, 1},
		{"old_on_new", modern, bridge.LegacySlotSchema, 0},
		{"unknown_on_new", modern, "unknown", 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			e := base
			e.Schema = tc.schema
			e.StartSlot = tc.start
			before, err := os.ReadFile(filepath.Join(tc.parent, slotMarker))
			if err != nil {
				t.Fatal(err)
			}
			if err = PublishSlot(ctx, tc.parent, pool.Version, "123/456", e); err == nil {
				t.Fatal("cross generation published")
			}
			if err = VerifySlotPublication(ctx, tc.parent, pool.Version, "123/456", e); err == nil {
				t.Fatal("cross generation verified")
			}
			after, err := os.ReadFile(filepath.Join(tc.parent, slotMarker))
			if err != nil || string(before) != string(after) {
				t.Fatal("rejection changed journal", err)
			}
		})
	}
}
