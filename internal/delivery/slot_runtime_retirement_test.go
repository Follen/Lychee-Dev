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

func runtimeRetirementFixture(t *testing.T, replaced bool) (string, bridge.SlotEnvelope, string) {
	t.Helper()
	ctx := context.Background()
	parent := t.TempDir()
	pool, err := installSlotPool(ctx, parent, "2.5.1")
	if err != nil {
		t.Fatal(err)
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("8", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	payload, err := bridge.SlotPayload(e)
	if err != nil {
		t.Fatal(err)
	}
	slot := &pool.Files[0]
	slot.PendingHash, slot.Nonce, slot.Runtime, slot.Consumer = slotDigest(payload), e.Nonce, e.Runtime, "123/456"
	if err = saveSlotPool(ctx, parent, pool); err != nil {
		t.Fatal(err)
	}
	wantHash := slot.PayloadHash
	if replaced {
		if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), payload, 0600); err != nil {
			t.Fatal(err)
		}
		wantHash = slot.PendingHash
	}
	return parent, e, wantHash
}

func TestRuntimeRetirementReconcilesTornPublication(t *testing.T) {
	for _, replaced := range []bool{false, true} {
		t.Run(fmt.Sprint(replaced), func(t *testing.T) {
			ctx := context.Background()
			parent, e, wantHash := runtimeRetirementFixture(t, replaced)
			next := strings.Repeat("1", 32) // Retirement does not infer chronology from token ordering.
			before, err := os.ReadFile(filepath.Join(parent, slotMarker))
			if err != nil {
				t.Fatal(err)
			}
			if err = RetireSlotRuntime(ctx, parent, "2.5.1", "foreign", e, next); err == nil {
				t.Fatal("foreign consumer retired")
			}
			unchanged, err := os.ReadFile(filepath.Join(parent, slotMarker))
			if err != nil {
				t.Fatal(err)
			}
			if string(before) != string(unchanged) {
				t.Fatal("foreign retirement mutated manifest")
			}
			for i := 0; i < 2; i++ {
				if err = RetireSlotRuntime(ctx, parent, "2.5.1", "123/456", e, next); err != nil {
					t.Fatal(err)
				}
			}
			pool, err := InspectSlots(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			s := pool.Files[0]
			if s.PayloadHash != wantHash || s.PendingHash != "" || s.RetiredRuntime != next || s.Consumed || s.RetiredProcess {
				t.Fatalf("incorrect retirement: %+v", s)
			}
			if err = RetireSlotRuntime(ctx, parent, "2.5.1", "123/456", e, strings.Repeat("9", 32)); err == nil {
				t.Fatal("different successor replaced persisted proof")
			}
			pool, err = InspectSlots(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			if pool.Files[0] != s {
				t.Fatal("conflicting proof mutated manifest")
			}
			replacement := e
			replacement.Runtime = next
			replacement.Nonce = strings.Repeat("5", 32)
			if err = PublishSlot(ctx, parent, "2.5.1", "another-instance", replacement); err != nil {
				t.Fatal(err)
			}
			pool, err = InspectSlots(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			s = pool.Files[0]
			if err = RetireSlotRuntime(ctx, parent, "2.5.1", "123/456", e, next); err != nil {
				t.Fatal(err)
			}
			pool, err = InspectSlots(ctx, parent, "2.5.1")
			if err != nil {
				t.Fatal(err)
			}
			if pool.Files[0] != s {
				t.Fatal("late retirement changed new nonce")
			}
		})
	}
}

func TestRuntimeRetirementRejectsCorruptPayload(t *testing.T) {
	parent, e, _ := runtimeRetirementFixture(t, true)
	path := filepath.Join(parent, slotMarker)
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), []byte("corrupt"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = RetireSlotRuntime(context.Background(), parent, "2.5.1", "123/456", e, strings.Repeat("1", 32)); err == nil {
		t.Fatal("corrupt payload retired")
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) {
		t.Fatal("corrupt payload changed manifest")
	}
}
