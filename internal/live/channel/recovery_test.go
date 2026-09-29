package channel

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type recoveryPeer struct {
	identity   Identity
	envelope   bridge.SlotEnvelope
	sends      int
	retireFail bool
}

func (b *recoveryPeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	return ObserveRecords(ctx, b, q)
}

func (b *recoveryPeer) Publish(_ context.Context, e bridge.SlotEnvelope) error {
	if e.Action != "bind" {
		panic("business input during recovery")
	}
	b.envelope = e
	return nil
}
func (b *recoveryPeer) Input(context.Context, InputAction) (InputOutcome, error) {
	b.sends++
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1}, nil
}
func (b *recoveryPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	return (&pendingBackend{}).ObserveInput(ctx, e, after, capability)
}
func (b *recoveryPeer) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	return &b.identity, nil
}
func (b *recoveryPeer) Consumed(context.Context, bridge.SlotEnvelope) error { return nil }
func (b *recoveryPeer) Supersede(context.Context, bridge.SlotEnvelope, Identity) error {
	if b.retireFail {
		b.retireFail = false
		return errors.New("injected retirement crash")
	}
	return nil
}
func (b *recoveryPeer) Find(context.Context, memory.Selector, bool) (memory.LookupResult, error) {
	i := b.identity
	i.Schema = b.envelope.Schema
	i.Owner = b.envelope.Owner
	i.Fence = b.envelope.Fence
	i.NextSlot = b.envelope.Index + 1
	r := Receipt{Identity: i, Nonce: b.envelope.Nonce, Ticket: b.envelope.Ticket, Action: "bind", State: "bound"}
	data, _ := json.Marshal(r)
	return memory.LookupResult{Records: []memory.Record{{Payload: data}}}, nil
}

func TestRuntimeRecoveryDistinguishesObservationFromOpaqueExecution(t *testing.T) {
	for _, policy := range []string{"observation", "opaque"} {
		t.Run(policy, func(t *testing.T) {
			ctx := context.Background()
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 5, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
			current := old
			current.Runtime = strings.Repeat("2", 32)
			current.NextSlot = 1
			peer := &recoveryPeer{identity: current, retireFail: true}
			d, err := New(filepath.Join(t.TempDir(), "connections", "test.jsonl"), peer, old)
			if err != nil {
				t.Fatal(err)
			}
			d.State.Bound = true
			if err = d.PrepareRequest(ctx, "same-key", "return 1", 5, policy); err != nil {
				t.Fatal(err)
			}
			op := d.State.Operation
			op.Stage = "running"
			op.PreparedNonce = strings.Repeat("3", 32)
			op.Challenge = strings.Repeat("4", 32)
			oldTicket, id := op.Ticket, op.ID
			if err = d.begin(ctx, "confirm", op.Ticket, nil); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "input_attempted"
			if err = d.Save(ctx, "before_reload"); err != nil {
				t.Fatal(err)
			}
			if err = d.RecoverRuntime(ctx, current); err == nil {
				t.Fatal("missing injected crash")
			}
			d, err = Load(d.Log, peer)
			if err != nil {
				t.Fatal(err)
			}
			err = d.finishRecovery(ctx)
			if policy == "opaque" {
				if !errors.Is(err, ErrExecutionUnknown) || d.State.Operation.Stage != "execution_unknown" || d.State.Operation.Ticket != oldTicket {
					t.Fatalf("opaque replay: %v %+v", err, d.State.Operation)
				}
			} else if err != nil || d.State.Operation.Stage != "prepared" || d.State.Operation.Ticket == oldTicket || d.State.Operation.Attempt != 2 {
				t.Fatalf("observation continuation: %v %+v", err, d.State.Operation)
			}
			if peer.sends != 1 || d.State.Operation.ID != id || d.State.Operation.Request != "same-key" {
				t.Fatal("recovery repeated binding or lost logical identity")
			}
			if _, err = Load(d.Log, peer); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestCommitPublicationWithoutNewInputIsStillExecutionUnknown(t *testing.T) {
	for _, phase := range []string{"intent", "published"} {
		t.Run(phase, func(t *testing.T) {
			ctx := context.Background()
			old := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 3, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
			current := old
			current.Runtime = strings.Repeat("2", 32)
			current.NextSlot = 1
			peer := &recoveryPeer{identity: current}
			d, err := New(filepath.Join(t.TempDir(), "connection.jsonl"), peer, old)
			if err != nil {
				t.Fatal(err)
			}
			d.State.Bound = true
			if err = d.PrepareRequest(ctx, "effect", "return 1", 5, "opaque"); err != nil {
				t.Fatal(err)
			}
			op := d.State.Operation
			op.Stage = "commit_ready"
			op.PreparedNonce = strings.Repeat("3", 32)
			op.Challenge = strings.Repeat("4", 32)
			if err = d.begin(ctx, "commit", op.Ticket, func(e *bridge.SlotEnvelope) { e.PreparedNonce = op.PreparedNonce; e.Challenge = op.Challenge }); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = phase
			if err = d.RecoverRuntime(ctx, current); !errors.Is(err, ErrExecutionUnknown) {
				t.Fatalf("published commit reclassified as not sent: %v", err)
			}
			if peer.sends != 1 || op.Attempt != 1 {
				t.Fatal("opaque operation replayed")
			}
		})
	}
}
