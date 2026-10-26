package delivery

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

const resetTestVersion = "3.2.5"

func slotResetFixture(t *testing.T) (client, parent, archive string) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	client = filepath.Join(root, "_retail_")
	parent = filepath.Join(client, "Interface", "AddOns")
	main := filepath.Join(parent, "Lychee Dev")
	if err = os.MkdirAll(filepath.Join(main, "Bridge"), 0700); err != nil {
		t.Fatal(err)
	}
	for path, raw := range map[string][]byte{
		filepath.Join(client, ".flavor.info"):            []byte("Product Flavor!STRING:0\nwow\n"),
		filepath.Join(client, "version.txt"):             []byte("12.1.0.69875\n"),
		filepath.Join(main, "Bridge", "SlotRuntime.lua"): []byte("-- managed reset fixture\n"),
	} {
		if err = os.WriteFile(path, raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
	raw := []byte("-- managed reset fixture\n")
	receipt := InstallationReceipt{Schema: "lycheedev.installation.v1", Component: "addon", Version: resetTestVersion, Commit: strings.Repeat("a", 40), Resources: []Resource{{Path: "addon/Bridge/SlotRuntime.lua", Bytes: int64(len(raw)), SHA256: slotDigest(raw)}}}
	encoded, err := json.Marshal(receipt)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(main, installationMarker), encoded, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = installSlotPool(context.Background(), parent, resetTestVersion); err != nil {
		t.Fatal(err)
	}
	return client, parent, filepath.Join(root, "reset-archive")
}

func assertResetInitialPool(t *testing.T, parent string) {
	t.Helper()
	pool, err := InspectSlots(context.Background(), parent, resetTestVersion)
	if err != nil || len(pool.Files) != bridge.SlotCount {
		t.Fatalf("rebuilt pool: %+v %v", pool, err)
	}
	for i, slot := range pool.Files {
		if slot != (SlotFile{PayloadHash: slotDigest([]byte(inertSlot))}) {
			t.Fatalf("slot %d retained old business state: %+v", i+1, slot)
		}
	}
}

func TestResetAddonSlotsMissingAndCorruptManifest(t *testing.T) {
	for _, missing := range []bool{true, false} {
		name := "corrupt"
		if missing {
			name = "missing"
		}
		t.Run(name, func(t *testing.T) {
			client, parent, archive := slotResetFixture(t)
			marker := filepath.Join(parent, slotMarker)
			original := []byte("{broken manifest\x00\xff\n")
			var err error
			if missing {
				err = os.Remove(marker)
			} else {
				err = os.WriteFile(marker, original, 0600)
			}
			if err != nil {
				t.Fatal(err)
			}
			unknown := []byte("unresolved original payload\x00\xff\n")
			if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), unknown, 0600); err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 2), "Loader.lua"), []byte("broken loader\n"), 0600); err != nil {
				t.Fatal(err)
			}
			if err = os.Remove(filepath.Join(SlotDirectory(parent, 3), "Payload.lua")); err != nil {
				t.Fatal(err)
			}
			last := SlotDirectory(parent, bridge.SlotCount)
			for name := range slotStatic(bridge.SlotCount, resetTestVersion) {
				if err = os.Remove(filepath.Join(last, name)); err != nil {
					t.Fatal(err)
				}
			}
			if err = os.Remove(filepath.Join(last, "Payload.lua")); err != nil {
				t.Fatal(err)
			}
			if err = os.Remove(last); err != nil {
				t.Fatal(err)
			}
			// Hard reset retains arbitrary original files inside fixed slot members.
			if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 4), "unknown.bin"), unknown, 0600); err != nil {
				t.Fatal(err)
			}
			other := filepath.Join(parent, "Other Addon")
			if err = os.Mkdir(other, 0700); err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(filepath.Join(other, "Keep.lua"), unknown, 0600); err != nil {
				t.Fatal(err)
			}
			result, err := ResetAddonSlots(context.Background(), client, archive, resetTestVersion)
			if err != nil || !result.Complete || result.Archive != archive {
				t.Fatalf("reset: %+v %v", result, err)
			}
			assertResetInitialPool(t, parent)
			for _, path := range []string{filepath.Join(SlotDirectory(archive, 1), "Payload.lua"), filepath.Join(SlotDirectory(archive, 4), "unknown.bin"), filepath.Join(other, "Keep.lua")} {
				got, readErr := os.ReadFile(path)
				if readErr != nil || !bytes.Equal(got, unknown) {
					t.Fatalf("original bytes changed at %s: %q %v", path, got, readErr)
				}
			}
			got, readErr := os.ReadFile(filepath.Join(archive, slotMarker))
			if missing {
				if !errors.Is(readErr, os.ErrNotExist) {
					t.Fatalf("missing original manifest was invented: %q %v", got, readErr)
				}
			} else if readErr != nil || !bytes.Equal(got, original) {
				t.Fatalf("corrupt original manifest changed: %q %v", got, readErr)
			}
			for _, path := range []string{SlotDirectory(archive, bridge.SlotCount), filepath.Join(SlotDirectory(archive, 3), "Payload.lua")} {
				if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("missing original was invented: %s %v", path, err)
				}
			}
		})
	}
}

type cancelResetAtPath struct {
	context.Context
	path string
}

func (c cancelResetAtPath) Err() error {
	if _, err := os.Lstat(c.path); err == nil {
		return context.Canceled
	}
	return c.Context.Err()
}

func TestResetAddonSlotsResumesArchiveAndInstall(t *testing.T) {
	for _, checkpoint := range []string{"archive", "install"} {
		t.Run(checkpoint, func(t *testing.T) {
			client, parent, archive := slotResetFixture(t)
			path := SlotDirectory(archive, 2)
			if checkpoint == "install" {
				if err := os.Remove(filepath.Join(parent, slotMarker)); err != nil {
					t.Fatal(err)
				}
			}
			ctx := context.Context(cancelResetAtPath{context.Background(), path})
			if checkpoint == "install" {
				ctx = cancelResetOnInstall{context.Background(), archive + ".slot-reset.json", parent}
			}
			result, err := ResetAddonSlots(ctx, client, archive, resetTestVersion)
			if !errors.Is(err, context.Canceled) || result.Complete {
				t.Fatalf("interruption: %+v %v", result, err)
			}
			result, err = ResetAddonSlots(context.Background(), client, archive, resetTestVersion)
			if err != nil || !result.Complete {
				t.Fatalf("resume: %+v %v", result, err)
			}
			assertResetInitialPool(t, parent)
		})
	}
}

type cancelResetOnInstall struct {
	context.Context
	intent, parent string
}

func (c cancelResetOnInstall) Err() error {
	raw, _ := os.ReadFile(c.intent)
	if bytes.Contains(raw, []byte(`"phase": "installing"`)) {
		if _, err := os.Lstat(filepath.Join(SlotDirectory(c.parent, 1), "Payload.lua")); err == nil {
			return context.Canceled
		}
	}
	return c.Context.Err()
}

func TestResetAddonSlotsPreservesPendingOriginalAndLaterBusiness(t *testing.T) {
	client, parent, archive := slotResetFixture(t)
	pool, err := readSlotPool(parent)
	if err != nil {
		t.Fatal(err)
	}
	pool.Files[0].Nonce = strings.Repeat("1", 32)
	pool.Files[0].PendingHash = strings.Repeat("2", 64)
	if err = saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	original, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil {
		t.Fatal(err)
	}
	if result, err := ResetAddonSlots(context.Background(), client, archive, resetTestVersion); err != nil || !result.Complete {
		t.Fatalf("reset: %+v %v", result, err)
	}
	archived, err := os.ReadFile(filepath.Join(archive, slotMarker))
	if err != nil || !bytes.Equal(archived, original) {
		t.Fatalf("original reservation changed: %q %v", archived, err)
	}
	pool, err = readSlotPool(parent)
	if err != nil {
		t.Fatal(err)
	}
	pool.Files[1].Nonce = strings.Repeat("3", 32)
	if err = saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	later, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil {
		t.Fatal(err)
	}
	if result, err := ResetAddonSlots(context.Background(), client, archive, resetTestVersion); err != nil || !result.Complete {
		t.Fatalf("completed retry: %+v %v", result, err)
	}
	got, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil || !bytes.Equal(got, later) {
		t.Fatalf("retry cleared later business: %q %v", got, err)
	}
	if err = os.WriteFile(filepath.Join(SlotDirectory(archive, 1), "Payload.lua"), []byte("archive changed"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = ResetAddonSlots(context.Background(), client, archive, resetTestVersion); !errors.Is(err, ErrConflict) {
		t.Fatal("completed retry ignored changed archive", err)
	}
	got, err = os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil || !bytes.Equal(got, later) {
		t.Fatalf("archive conflict changed later business: %q %v", got, err)
	}
}

func TestResetAddonSlotsRefusesWindowClaimWithoutMutation(t *testing.T) {
	client, parent, archive := slotResetFixture(t)
	owner := journal.WindowOwner{WorkspaceID: strings.Repeat("a", 32), Resource: "window/11/22/33", OperationID: "CON-" + strings.Repeat("b", 32), IntentSHA256: strings.Repeat("c", 64)}
	if err := journal.BeginConnectionWindow(context.Background(), parent, owner, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil {
		t.Fatal(err)
	}
	result, err := ResetAddonSlots(context.Background(), client, archive, resetTestVersion)
	if !errors.Is(err, journal.ErrBusy) || result.Complete {
		t.Fatalf("claim was ignored: %+v %v", result, err)
	}
	after, err := os.ReadFile(filepath.Join(parent, slotMarker))
	if err != nil || !bytes.Equal(before, after) {
		t.Fatalf("busy reset changed pool: %v", err)
	}
	assertResetInitialPool(t, parent)
	for _, path := range []string{archive, archive + ".slot-reset.json"} {
		if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("busy reset created archive: %s %v", path, err)
		}
	}
	actual, occupied, err := journal.InspectWindowOwner(context.Background(), parent, owner.Resource)
	if err != nil || !occupied || actual.OperationID != owner.OperationID {
		t.Fatalf("reset changed the existing claim: %+v %t %v", actual, occupied, err)
	}
}

func TestResetAddonSlotsRejectsMismatchedArchiveAndReceipt(t *testing.T) {
	client, parent, archive := slotResetFixture(t)
	if err := os.Mkdir(archive, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := ResetAddonSlots(context.Background(), client, archive, resetTestVersion); !errors.Is(err, ErrConflict) {
		t.Fatal("adopted existing archive", err)
	}
	if _, err := ResetAddonSlots(context.Background(), client, archive+"-wrong-version", "0.0.0"); !errors.Is(err, ErrConflict) {
		t.Fatal("accepted mismatched receipt version", err)
	}
	if _, err := ResetAddonSlots(context.Background(), client, filepath.Join(parent, "archive"), resetTestVersion); !errors.Is(err, ErrInstallation) {
		t.Fatal("accepted archive inside AddOns", err)
	}
	if err := os.WriteFile(filepath.Join(parent, "Lychee Dev", "Bridge", "SlotRuntime.lua"), []byte("external main addon edit"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := ResetAddonSlots(context.Background(), client, archive+"-modified-main", resetTestVersion); !errors.Is(err, ErrConflict) {
		t.Fatal("reset alongside modified main addon", err)
	}
	assertResetInitialPool(t, parent)
}
