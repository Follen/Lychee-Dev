package delivery

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestForwardAllocationIsBoundedAndPreservesUnknown(t *testing.T) {
	p := SlotPool{Files: make([]SlotFile, bridge.SlotCount)}
	for i := range p.Files {
		p.Files[i] = SlotFile{Nonce: "held"}
	}
	if NextFreeSlot(p, 1) != 0 {
		t.Fatal("allocated unknown reservation")
	}
	p.Files[199].Consumed = true
	if NextFreeSlot(p, 1) != 200 {
		t.Fatal("last slot unavailable")
	}
	p.Files[199].PendingHash = "interrupted"
	if NextFreeSlot(p, 1) != 0 {
		t.Fatal("allocated incomplete publication")
	}
	p.Files[0] = SlotFile{}
	if NextFreeSlot(p, 1) != 0 {
		t.Fatal("allocation moved backwards")
	}
}

func TestRouteChecksActualForeignAndEmptyBytes(t *testing.T) {
	parent := t.TempDir()
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	foreign, err := bridge.SlotPayload(e)
	if err != nil {
		t.Fatal(err)
	}
	dir := SlotDirectory(parent, 1)
	if err = os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	e.Index, e.StartSlot, e.Runtime = 2, 1, strings.Repeat("5", 32)
	for _, tc := range []struct {
		name    string
		bytes   []byte
		allowed bool
	}{
		{"empty", []byte(inertSlot), true}, {"foreign", foreign, true},
		{"same_runtime", []byte(strings.ReplaceAll(string(foreign), strings.Repeat("1", 32), e.Runtime)), false},
		{"legacy", []byte(strings.ReplaceAll(string(foreign), bridge.SlotSchema, bridge.LegacySlotSchema)), false},
		{"wrong_slot", []byte(strings.ReplaceAll(string(foreign), "index = 1,", "index = 7,")), false},
		{"unknown_action", []byte(strings.ReplaceAll(string(foreign), "\"bind\"", "\"other\"")), false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if err := os.WriteFile(filepath.Join(dir, "Payload.lua"), tc.bytes, 0600); err != nil {
				t.Fatal(err)
			}
			err := VerifySlotRoute(context.Background(), parent, e)
			if (err == nil) != tc.allowed {
				t.Fatalf("route=%v allowed=%v", err, tc.allowed)
			}
		})
	}
}
