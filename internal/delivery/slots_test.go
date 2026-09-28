package delivery

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestSlotPoolInstallPublicationAndIsolation(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	pool, err := installSlotPool(ctx, parent, "2.5.1")
	if err != nil {
		t.Fatal(err)
	}
	if pool.State != "ready" || len(pool.Files) != 64 {
		t.Fatalf("%+v", pool)
	}
	if _, err = InspectSlots(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	if err = PublishSlot(ctx, parent, "2.5.1", "instance-a", e); err != nil {
		t.Fatal(err)
	}
	other := e
	other.Nonce = strings.Repeat("5", 32)
	other.Runtime = strings.Repeat("6", 32)
	if err = PublishSlot(ctx, parent, "2.5.1", "instance-b", other); err == nil {
		t.Fatal("overwrote uncertain publication")
	}
	if err = ConfirmSlotConsumed(ctx, parent, "2.5.1", "instance-b", 1, e.Nonce); err == nil {
		t.Fatal("wrong consumer confirmed")
	}
	if err = ConfirmSlotConsumed(ctx, parent, "2.5.1", "instance-a", 1, e.Nonce); err != nil {
		t.Fatal(err)
	}
	if err = PublishSlot(ctx, parent, "2.5.1", "instance-b", other); err != nil {
		t.Fatal(err)
	}
	if _, err = InspectSlots(ctx, parent, "2.5.1"); err != nil {
		t.Fatal("authorized payload marked modified:", err)
	}
	if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 2), "Loader.lua"), []byte("-- edited"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = InspectSlots(ctx, parent, "2.5.1"); err == nil {
		t.Fatal("static drift ignored")
	}
}

func TestEndedProcessRetirementReconcilesTornPublication(t *testing.T) {
	for _, replaced := range []bool{false, true} {
		t.Run(fmt.Sprint(replaced), func(t *testing.T) {
			ctx := context.Background()
			parent := t.TempDir()
			pool, err := installSlotPool(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
			b, err := bridge.SlotPayload(e)
			if err != nil {
				t.Fatal(err)
			}
			pool.Files[0].PendingHash = slotDigest(b)
			pool.Files[0].Nonce = e.Nonce
			pool.Files[0].Runtime = e.Runtime
			pool.Files[0].Consumer = "123/456"
			if err = saveSlotPool(ctx, parent, pool); err != nil {
				t.Fatal(err)
			}
			if replaced {
				if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), b, 0600); err != nil {
					t.Fatal(err)
				}
			}
			if err = RetireSlotProcess(ctx, parent, "2.5.1", "other", e); err == nil {
				t.Fatal("foreign consumer retired")
			}
			for j := 0; j < 2; j++ {
				if err = RetireSlotProcess(ctx, parent, "2.5.1", "123/456", e); err != nil {
					t.Fatal(err)
				}
			}
			pool, err = InspectSlots(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			if pool.Files[0].Consumed || !pool.Files[0].RetiredProcess || pool.Files[0].PendingHash != "" {
				t.Fatal("fabricated receipt or incomplete retirement")
			}
			e.Nonce = strings.Repeat("5", 32)
			if err = PublishSlot(ctx, parent, "2.5.1", "new", e); err != nil {
				t.Fatal(err)
			}
			pool, err = InspectSlots(ctx, parent, "2.5.1")
			if err != nil || pool.Files[0].RetiredProcess {
				t.Fatal("inherited retirement", err)
			}
		})
	}
}

func TestRuntimeRetirementNeverClaimsConsumptionOrTouchesAnotherProcess(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	if _, err := installSlotPool(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	if err := PublishSlot(ctx, parent, "2.5.1", "instance-a", e); err != nil {
		t.Fatal(err)
	}
	newRuntime := strings.Repeat("5", 32)
	if err := RetireSlotRuntime(ctx, parent, "2.5.1", "instance-b", e, newRuntime); err == nil {
		t.Fatal("retired foreign reservation")
	}
	if err := RetireSlotRuntime(ctx, parent, "2.5.1", "instance-a", e, newRuntime); err != nil {
		t.Fatal(err)
	}
	pool, err := InspectSlots(ctx, parent, "2.5.1")
	if err != nil {
		t.Fatal(err)
	}
	if pool.Files[0].Consumed || pool.Files[0].RetiredRuntime != newRuntime {
		t.Fatal("fabricated consumption")
	}
	e.Runtime = newRuntime
	e.Nonce = strings.Repeat("6", 32)
	if err = PublishSlot(ctx, parent, "2.5.1", "instance-b", e); err != nil {
		t.Fatal(err)
	}
	pool, err = InspectSlots(ctx, parent, "2.5.1")
	if err != nil || pool.Files[0].RetiredRuntime != "" {
		t.Fatal("new reservation inherited retirement", err)
	}
}
func TestSlotPoolIncompleteInstallAndPendingPublication(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	pool := SlotPool{Schema: "lycheedev.slots.v1", Version: "2.5.1", State: "installing", Files: make([]SlotFile, 64)}
	if err := saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	if _, err := InspectSlots(ctx, parent, "2.5.1"); err == nil {
		t.Fatal("partial installation accepted")
	}
	if _, err := installSlotPool(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	pool, err := readSlotPool(parent)
	if err != nil {
		t.Fatal(err)
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	payload, err := bridge.SlotPayload(e)
	if err != nil {
		t.Fatal(err)
	}
	pool.Files[0].PendingHash = slotDigest(payload)
	pool.Files[0].Nonce = e.Nonce
	pool.Files[0].Runtime = e.Runtime
	pool.Files[0].Consumer = "a"
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	// Crash before payload rename; retry the same intent, not another nonce.
	if err = PublishSlot(ctx, parent, "2.5.1", "a", e); err != nil {
		t.Fatal(err)
	}
	pool, err = InspectSlots(ctx, parent, "2.5.1")
	if err != nil || pool.Files[0].PendingHash != "" {
		t.Fatalf("%+v %v", pool, err)
	}
}
func TestSlotPoolDoesNotAdoptUnmanagedDirectories(t *testing.T) {
	parent := t.TempDir()
	if err := os.Mkdir(SlotDirectory(parent, 1), 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := installSlotPool(context.Background(), parent, "2.5.1"); err == nil {
		t.Fatal("adopted unmanaged slot")
	}
}

func TestSlotBootstrapRepairPreservesPayloadAndRejectsDrift(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	if _, err := installSlotPool(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	marker, _ := os.ReadFile(filepath.Join(parent, slotMarker))
	path := filepath.Join(SlotDirectory(parent, 1), "Lychee Dev Slot 01.toc")
	want, _ := os.ReadFile(path)
	if strings.Contains(string(want), "Dependencies:") {
		t.Fatal("slot must be passive without a main-addon dependency")
	}
	old := strings.Replace(string(want), "## LoadOnDemand: 1\n", "## LoadOnDemand: 1\n## Dependencies: Lychee Dev\n", 1)
	if err := os.WriteFile(path, []byte(old), 0600); err != nil {
		t.Fatal(err)
	}
	if err := RepairSlotBootstrap(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	after, _ := os.ReadFile(filepath.Join(parent, slotMarker))
	if string(marker) != string(after) {
		t.Fatal("repair changed reservation ledger")
	}
	if err := RepairSlotBootstrap(ctx, parent, "2.5.1"); err != nil {
		t.Fatal("not resumable", err)
	}
	if err := os.WriteFile(path, []byte(old+"Injected.lua\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := RepairSlotBootstrap(ctx, parent, "2.5.1"); err == nil {
		t.Fatal("repair accepted unknown drift")
	}
}

func TestSlotPoolRejectsUnexpectedFiles(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	if _, err := installSlotPool(ctx, parent, "2.5.1"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 4), "Other.lua"), []byte("return"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := InspectSlots(ctx, parent, "2.5.1"); err == nil {
		t.Fatal("unmanaged slot file ignored")
	}
}
