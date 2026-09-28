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

func TestInputObservationRejectsStaleAndOtherTargets(t *testing.T) {
	blocked := false
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, Index: 3, GUID: "g", Build: "b"}
	base := InputObservation{Schema: "lycheedev.input.v1", Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: 3, GUID: "g", Build: "b", SampleMillis: 1200, InputBlocked: &blocked}
	for _, tc := range []struct {
		name  string
		edit  func(*InputObservation)
		valid bool
	}{
		{"fresh", func(*InputObservation) {}, true},
		{"before-input", func(s *InputObservation) { s.SampleMillis = 1000 }, false},
		{"clock-ahead", func(s *InputObservation) { s.SampleMillis = 99999 }, false},
		{"old-runtime", func(s *InputObservation) { s.Runtime = strings.Repeat("2", 32) }, false},
		{"different-owner", func(s *InputObservation) { s.Owner = "other" }, false},
		{"different-slot", func(s *InputObservation) { s.NextSlot = 4 }, false},
		{"missing-block", func(s *InputObservation) { s.InputBlocked = nil }, false},
		{"conflicting-reason", func(s *InputObservation) { s.Reason = "input_keyboard_focus" }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := base
			tc.edit(&s)
			b, _ := json.Marshal(s)
			_, err := inputObservation(memory.Record{Header: bridge.MemoryHeader{Kind: bridge.MemoryInputState}, Payload: b}, e, 1000, 1250)
			if (err == nil) != tc.valid {
				t.Fatal(err)
			}
		})
	}
}

type focusBackend struct {
	pendingBackend
	blocked bool
	before  int
}

func (b *focusBackend) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64) (InputObservation, error) {
	b.before++
	s, _ := b.pendingBackend.ObserveInput(ctx, e, after)
	s.InputBlocked = &b.blocked
	if b.blocked {
		s.Reason = "input_keyboard_focus"
	}
	return s, nil
}
func (b *focusBackend) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	if a.Kind == "escape" {
		return InputOutcome{Disposition: "submitted", MessagesQueued: 2}, nil
	}
	return b.pendingBackend.Input(ctx, a)
}
func TestEscRecoveryPersistsBeforeWakeAndResumesOriginalSlot(t *testing.T) {
	b := &focusBackend{blocked: true}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "c.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	nonce := d.State.Transaction.Envelope.Nonce
	if b.sends != 0 || d.State.Transaction.Phase != "published" || d.State.Input.Kind != "escape" || d.State.Input.Outcome == nil {
		t.Fatal("Esc preflight was labelled as slot input")
	}
	d, err = Load(d.Log, b)
	if err != nil {
		t.Fatal(err)
	}
	b.blocked = false
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if b.sends != 1 || b.publishes != 1 || d.State.Transaction.Envelope.Nonce != nonce {
		t.Fatal("recovery replaced/replayed the operation")
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if b.sends != 1 || b.before != 2 {
		t.Fatal("unknown slot input was retried")
	}
}

type zeroInputBackend struct {
	pendingBackend
	zero bool
}

func (b *zeroInputBackend) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	if b.zero {
		b.zero = false
		return InputOutcome{Disposition: "not_sent", Retryable: true, Reason: "key_held"}, nil
	}
	return b.pendingBackend.Input(ctx, a)
}
func TestProvenZeroInputResumesSameSlot(t *testing.T) {
	b := &zeroInputBackend{zero: true}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "c.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	nonce := d.State.Transaction.Envelope.Nonce
	if d.State.Transaction.Phase != "published" || b.sends != 0 {
		t.Fatal("zero input became uncertain")
	}
	d, err = Load(d.Log, b)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if b.sends != 1 || b.publishes != 1 || d.State.Transaction.Envelope.Nonce != nonce {
		t.Fatal("slot changed")
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if b.sends != 1 {
		t.Fatal("uncertain input replayed")
	}
}

func TestMissingInputOutcomeNeverAuthorizesResend(t *testing.T) {
	b := &pendingBackend{}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "c.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.begin(context.Background(), "bind", d.State.Owner, nil); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "published"
	d.State.Input = &InputAttempt{ID: strings.Repeat("2", 32), Runtime: i.Runtime, Exchange: d.State.Transaction.Envelope.Nonce, Kind: "invoke"}
	if err = d.Save(context.Background(), "input_intent"); err != nil {
		t.Fatal(err)
	}
	d, err = Load(d.Log, b)
	if err != nil {
		t.Fatal(err)
	}
	if err = d.Connect(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if b.sends != 0 || b.publishes != 0 || b.reads != 1 {
		t.Fatal("uncertain input replay", b)
	}
}
