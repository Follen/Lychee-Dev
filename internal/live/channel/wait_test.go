package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type pendingBackend struct {
	publishes, sends, reads int
	cancel                  context.CancelFunc
}

func (b *pendingBackend) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	return ObserveRecords(ctx, b, q)
}

func (b *pendingBackend) Publish(context.Context, bridge.SlotEnvelope) error {
	b.publishes++
	return nil
}
func (b *pendingBackend) Input(context.Context, InputAction) (InputOutcome, error) {
	b.sends++
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1}, nil
}
func (b *pendingBackend) ObserveInput(_ context.Context, e bridge.SlotEnvelope, _ int64, _ string) (InputObservation, error) {
	ready := false
	return InputObservation{Schema: "lycheedev.input.v1", Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, InputBlocked: &ready}, nil
}
func (*pendingBackend) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	return nil, nil
}
func (*pendingBackend) Supersede(context.Context, bridge.SlotEnvelope, Identity) error { return nil }
func (b *pendingBackend) Consumed(context.Context, bridge.SlotEnvelope) error {
	panic("unverified receipt")
}
func (b *pendingBackend) Find(context.Context, memory.Selector, bool) (memory.LookupResult, error) {
	b.reads++
	if b.cancel != nil {
		b.cancel()
	}
	return memory.LookupResult{}, nil
}

func TestBoundedContinuationDoesNotRepeatUnknownInput(t *testing.T) {
	b := &pendingBackend{}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "connection.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Continue(context.Background()); err == nil {
		t.Fatal("unbounded wait accepted")
	}
	for attempt := 0; attempt < 2; attempt++ {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		b.cancel = cancel
		err = d.Continue(ctx)
		cancel()
		if !errors.Is(err, ErrPending) {
			t.Fatal(err)
		}
		d, err = Load(d.Log, b)
		if err != nil {
			t.Fatal(err)
		}
	}
	if b.publishes != 1 || b.sends != 1 || b.reads != 2 {
		t.Fatalf("%+v", b)
	}
}

type publicationBlockedBackend struct {
	pendingBackend
	candidates int
}

func (b *publicationBlockedBackend) Publish(context.Context, bridge.SlotEnvelope) error {
	b.publishes++
	b.cancel()
	return ErrPublicationPending
}
func (b *publicationBlockedBackend) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	b.candidates++
	return nil, nil
}
func TestPublicationWaitRetainsIntentWithoutInputOrRuntimeDiscovery(t *testing.T) {
	b := &publicationBlockedBackend{}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "connection.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	var nonce string
	for attempt := 0; attempt < 2; attempt++ {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		b.cancel = cancel
		err = d.Continue(ctx)
		cancel()
		if !errors.Is(err, ErrPending) || d.Waiting != "shared_publication" {
			t.Fatalf("waiting=%s err=%v", d.Waiting, err)
		}
		d, err = Load(d.Log, b)
		if err != nil {
			t.Fatal(err)
		}
		if d.State.Transaction == nil || d.State.Transaction.Phase != "intent" {
			t.Fatal("lost publication intent")
		}
		if attempt == 0 {
			nonce = d.State.Transaction.Envelope.Nonce
		} else if d.State.Transaction.Envelope.Nonce != nonce {
			t.Fatal("reallocated nonce")
		}
	}
	if b.sends != 0 || b.reads != 0 || b.candidates != 0 {
		t.Fatalf("unexpected effects: %+v", b)
	}
}
