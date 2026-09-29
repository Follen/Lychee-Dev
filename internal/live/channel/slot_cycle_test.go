//go:build windows && amd64

package channel

import (
	"context"
	"errors"
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

// Characterization, not a claim of successful recovery: an unknown bootstrap
// reservation can prevent the very fresh bind required to retire it. A foreign
// instance's unknown bootstrap also blocks an otherwise recoverable connection.
func TestSlotCycleRecoveryWaitsForUnknownSlotOne(t *testing.T) {
	for _, foreign := range []bool{false, true} {
		name := "own_unconfirmed_bootstrap"
		if foreign {
			name = "foreign_instance_bootstrap"
		}
		t.Run(name, func(t *testing.T) {
			n, reserved := publicationFixture(t)
			// Installation setup is not part of this protocol-behavior deadline.
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: n.Version}
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
			consumer := n.Consumer
			reserved = original
			if foreign {
				// Same installation and build, different PID/start-time identity.
				consumer = "2/2"
				reserved.Index, reserved.Runtime, reserved.Nonce = 1, strings.Repeat("6", 32), strings.Repeat("7", 32)
				reserved.Owner = strings.Repeat("9", 32)
				other := &Native{Parent: n.Parent, Version: n.Version, Consumer: consumer, Guard: n.Guard}
				if err = other.Publish(ctx, reserved); err != nil {
					t.Fatal(err)
				}
			}
			path := filepath.Join(delivery.SlotDirectory(n.Parent, 1), "Payload.lua")
			before, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if err = d.RecoverRuntime(ctx, current); !errors.Is(err, ErrPublicationPending) {
				t.Fatalf("recovery error = %v", err)
			}
			if d.State.Recovery.Phase != "binding" || d.State.Bound || d.State.Transaction.Envelope.Index != 1 || d.State.Transaction.Phase != "intent" {
				t.Fatalf("unexpected recovery state: %+v", d.State)
			}
			// Check the unmapped storage cause under its real publication lease.
			if err = n.lockPublication(ctx); err != nil {
				t.Fatal(err)
			}
			publishErr := delivery.PublishSlot(ctx, n.Parent, n.Version, n.Consumer, d.State.Transaction.Envelope)
			if err = n.unlockPublication(); err != nil {
				t.Fatal(err)
			}
			if !errors.Is(publishErr, delivery.ErrSlotReserved) {
				t.Fatalf("storage cause = %v", publishErr)
			}
			bindNonce := d.State.Transaction.Envelope.Nonce
			for attempt := 0; attempt < 2; attempt++ {
				d, err = Load(d.Log, peer)
				if err != nil {
					t.Fatal(err)
				}
				if err = d.finishRecovery(ctx); !errors.Is(err, ErrPublicationPending) {
					t.Fatalf("resume = %v", err)
				}
				if d.State.Transaction.Envelope.Nonce != bindNonce {
					t.Fatal("resume changed bind nonce")
				}
			}
			after, err := os.ReadFile(path)
			if err != nil || string(before) != string(after) {
				t.Fatal("unknown slot overwritten", err)
			}
			if peer.sends != 0 || peer.retireCalls != 0 || n.Publication != nil {
				t.Fatal("input/retirement occurred, or OS lease was retained")
			}
			unrelated := reserved
			unrelated.Index, unrelated.Nonce = 2, strings.Repeat("a", 32)
			otherSlot := &Native{Parent: n.Parent, Version: n.Version, Consumer: "3/3", Guard: n.Guard}
			if err = otherSlot.Publish(ctx, unrelated); err != nil {
				t.Fatal("unrelated slot blocked by slot 1 reservation", err)
			}
			pool, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
			if err != nil || pool.Files[0].Consumer != consumer || pool.Files[0].Nonce != reserved.Nonce || pool.Files[0].Consumed || pool.Files[0].RetiredRuntime != "" {
				t.Fatal("reservation changed", err)
			}
			t.Log("confirmed: recovery binding waits for delivery.slot_reserved; repeated journal resume cannot reach retirement; no OS lease retained")
			if foreign {
				// Control: model the OTHER driver finally obtaining its exact receipt.
				// This is external progress, not permission for recovery to invent it.
				other := &Native{Parent: n.Parent, Version: n.Version, Consumer: consumer, Guard: n.Guard}
				if err = other.Consumed(ctx, reserved); err != nil {
					t.Fatal(err)
				}
				if err = d.finishRecovery(ctx); err != nil {
					t.Fatal("foreign receipt did not unblock recovery", err)
				}
				if !d.State.Bound || d.State.Recovery.Phase != "complete" || peer.sends != 1 || peer.retireCalls != 1 {
					t.Fatal("control did not complete exactly one bind and retirement")
				}
			}
		})
	}
}

func TestSlotCycleRecoveryFreeSlotOneControl(t *testing.T) {
	n, _ := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 5, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: n.Version}
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
