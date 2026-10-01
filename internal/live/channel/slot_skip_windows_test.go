//go:build windows && amd64

package channel

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
)

func TestSlotSkipForeignReservationPreservesOwnerAndResume(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	a, reserved := publicationFixture(t)
	if err := a.Publish(ctx, reserved); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(delivery.SlotDirectory(a.Parent, 1), "Payload.lua")
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("8", 32), NextSlot: 1, Slots: bridge.SlotCount, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: a.Version}
	b := &Native{Parent: a.Parent, Version: a.Version, Consumer: "2/2", Guard: a.Guard}
	p := &slotCyclePeer{recoveryPeer: recoveryPeer{identity: i}, native: b}
	d, err := New(filepath.Join(t.TempDir(), "connections", "skip.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	if !d.State.Bound || p.envelope.Index != 2 || p.envelope.StartSlot != 1 || p.sends != 1 {
		t.Fatalf("did not skip exactly once: %+v, sends=%d", d.State, p.sends)
	}
	if err = delivery.VerifySlotRoute(ctx, a.Parent, p.envelope); err != nil {
		t.Fatal(err)
	}
	after, err := os.ReadFile(path)
	if err != nil || string(after) != string(before) {
		t.Fatal("foreign payload changed", err)
	}
	pool, err := delivery.InspectSlots(ctx, a.Parent, a.Version)
	if err != nil {
		t.Fatal(err)
	}
	if pool.Files[0].Consumed || pool.Files[0].Nonce != reserved.Nonce || pool.Files[0].Consumer != a.Consumer {
		t.Fatal("foreign reservation retired")
	}
	d, err = Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(ctx); err != nil || p.sends != 1 {
		t.Fatal("completed bind replayed", err)
	}
}

func TestSlotSkipInputObservationRetainsRouteOrigin(t *testing.T) {
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 200, StartSlot: 1}
	if slotStart(e) != 1 {
		t.Fatal("route origin lost")
	}
	e.StartSlot = 0
	if slotStart(e) != 200 {
		t.Fatal("legacy exact-index gate lost")
	}
}
