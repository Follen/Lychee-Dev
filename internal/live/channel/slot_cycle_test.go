//go:build windows && amd64

package channel

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
)

// The game observations/input are simulated; publication, reservations,
// consumption, retirement and the recovery journal use production code.
type slotCyclePeer struct {
	recoveryPeer
	native      *Native
	retireCalls int
}

func (p *slotCyclePeer) Publish(ctx context.Context, e bridge.SlotEnvelope) error {
	if err := p.native.Publish(ctx, e); err != nil {
		return err
	}
	return p.recoveryPeer.Publish(ctx, e)
}
func (p *slotCyclePeer) Consumed(ctx context.Context, e bridge.SlotEnvelope) error {
	return p.native.Consumed(ctx, e)
}
func (p *slotCyclePeer) Supersede(ctx context.Context, e bridge.SlotEnvelope, i Identity) error {
	p.retireCalls++
	return p.native.Supersede(ctx, e, i)
}

// Production publication and journal recovery; game input/receipts are adapted.
func TestSlotCycleRecoverySkipsUnknownSlotOne(t *testing.T) {
	for _, foreign := range []bool{false, true} {
		t.Run(fmt.Sprint(foreign), func(t *testing.T) {
			n, reserved := publicationFixture(t)
			ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
			defer cancel()
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: n.Version}
			if foreign {
				old.NextSlot = 5
			}
			current := old
			current.Runtime, current.NextSlot = strings.Repeat("8", 32), 1
			peer := &slotCyclePeer{recoveryPeer: recoveryPeer{identity: current}, native: n}
			d, err := New(filepath.Join(t.TempDir(), "connections", "cycle.jsonl"), peer, old)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.begin(ctx, "bind", d.State.Owner, nil); err != nil {
				t.Fatal(err)
			}
			original := d.State.Transaction.Envelope
			if err = n.Publish(ctx, original); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "input_attempted"
			reserved = original
			if foreign {
				reserved.Index, reserved.StartSlot, reserved.Runtime, reserved.Nonce = 1, 1, strings.Repeat("6", 32), strings.Repeat("7", 32)
				other := &Native{Parent: n.Parent, Version: n.Version, Consumer: "2/2", Guard: n.Guard}
				if err = other.Publish(ctx, reserved); err != nil {
					t.Fatal(err)
				}
			}
			path := filepath.Join(delivery.SlotDirectory(n.Parent, 1), "Payload.lua")
			before, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.RecoverRuntime(ctx, current); err != nil {
				t.Fatal(err)
			}
			if !d.State.Bound || d.State.Recovery.Phase != "complete" || peer.envelope.Index != 2 || peer.sends != 1 || peer.retireCalls != 1 {
				t.Fatalf("failed recovery: %+v", d.State)
			}
			after, err := os.ReadFile(path)
			if err != nil || string(before) != string(after) {
				t.Fatal("old payload overwritten", err)
			}
			pool, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
			if err != nil {
				t.Fatal(err)
			}
			if foreign && (pool.Files[0].Consumed || pool.Files[0].RetiredRuntime != "" || pool.Files[0].Nonce != reserved.Nonce) {
				t.Fatal("foreign reservation retired")
			}
			d, err = Load(d.Log, peer)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.finishRecovery(ctx); err != nil || peer.sends != 1 || peer.retireCalls != 1 {
				t.Fatal("completed recovery replayed", err)
			}
		})
	}
}

func TestSlotCycleRecoveryFreeSlotOneControl(t *testing.T) {
	n, _ := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 5, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: n.Version}
	current := old
	current.Runtime, current.NextSlot = strings.Repeat("8", 32), 1
	peer := &slotCyclePeer{recoveryPeer: recoveryPeer{identity: current}, native: n}
	d, err := New(filepath.Join(t.TempDir(), "connections", "control.jsonl"), peer, old)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.begin(ctx, "bind", d.State.Owner, nil); err != nil {
		t.Fatal(err)
	}
	if err = n.Publish(ctx, d.State.Transaction.Envelope); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "input_attempted"
	if err = d.RecoverRuntime(ctx, current); err != nil {
		t.Fatal(err)
	}
	if !d.State.Bound || d.State.Recovery.Phase != "complete" || peer.sends != 1 || peer.retireCalls != 1 {
		t.Fatal("free slot 1 failed to complete exactly one bind and retirement")
	}
}
