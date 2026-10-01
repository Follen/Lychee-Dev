package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func inputTestRecord(t *testing.T, s InputObservation) memory.Record {
	t.Helper()
	runtime, err := tokenBytes(s.Runtime)
	if err != nil {
		t.Fatal(err)
	}
	payload, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Sequence: 1, Runtime: runtime, Nonce: runtime}, payload)
	if err != nil {
		t.Fatal(err)
	}
	header, payload, err := bridge.DecodeMemoryRecord(wire)
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Address: 123, Header: header, Payload: payload}
}

func TestInputObservationRejectsMalformedPublicationHeader(t *testing.T) {
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, Index: 3, GUID: "g", Build: "b"}
	blocked := false
	s := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, SampleMillis: 1200, InputBlocked: &blocked}
	base := inputTestRecord(t, s)
	other, _ := tokenBytes(strings.Repeat("2", 32))
	for _, tc := range []struct {
		name string
		edit func(*bridge.MemoryHeader)
	}{
		{"other-kind", func(h *bridge.MemoryHeader) { h.Kind = bridge.MemoryReceipt }},
		{"other-state", func(h *bridge.MemoryHeader) { h.State = 2 }},
		{"zero-sequence", func(h *bridge.MemoryHeader) { h.Sequence = 0 }},
		{"exhausted-sequence", func(h *bridge.MemoryHeader) { h.Sequence = ^uint32(0) }},
		{"nonzero-ticket", func(h *bridge.MemoryHeader) { h.Ticket = other }},
		{"other-runtime", func(h *bridge.MemoryHeader) { h.Runtime = other }},
		{"other-nonce", func(h *bridge.MemoryHeader) { h.Nonce = other }},
		{"zero-nonce", func(h *bridge.MemoryHeader) { h.Nonce = [16]byte{} }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			header := base.Header
			tc.edit(&header)
			// Encode/decode establishes checksum-valid malformed publication,
			// rather than relying on corrupt bytes rejected by the wire decoder.
			wire, err := bridge.EncodeMemoryRecord(header, base.Payload)
			if err != nil {
				t.Fatal(err)
			}
			header, payload, err := bridge.DecodeMemoryRecord(wire)
			if err != nil {
				t.Fatal("malformed qualification should still be legal wire", err)
			}
			if _, err := inputObservation(memory.Record{Header: header, Payload: payload}, e, 1000, 1250); err == nil {
				t.Fatal("checksum-valid malformed INPUT authorized")
			}
		})
	}
	for _, length := range []int{2048, 2049} {
		t.Run(fmt.Sprintf("payload-%d", length), func(t *testing.T) {
			payload := append(bytes.Clone(base.Payload), bytes.Repeat([]byte{' '}, length-len(base.Payload))...)
			wire, err := bridge.EncodeMemoryRecord(base.Header, payload)
			if err != nil {
				t.Fatal(err)
			}
			header, payload, err := bridge.DecodeMemoryRecord(wire)
			if err != nil {
				t.Fatal(err)
			}
			_, err = inputObservation(memory.Record{Header: header, Payload: payload}, e, 1000, 1250)
			if (err == nil) != (length == 2048) {
				t.Fatalf("payload length=%d error=%v", length, err)
			}
		})
	}
}

func TestInputObservationRejectsStaleAndOtherTargets(t *testing.T) {
	blocked := false
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, Index: 3, GUID: "g", Build: "b"}
	base := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: 3, GUID: "g", Build: "b", SampleMillis: 1200, InputBlocked: &blocked}
	for _, tc := range []struct {
		name  string
		edit  func(*InputObservation)
		valid bool
	}{
		{"fresh", func(*InputObservation) {}, true},
		{"old-schema", func(s *InputObservation) { s.Schema = "lycheedev.input.v1" }, false},
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
			_, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250)
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

func (b *focusBackend) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	b.before++
	s, _ := b.pendingBackend.ObserveInput(ctx, e, after, capability)
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
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
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
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
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
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
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
