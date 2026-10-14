package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Use the real INPUT decoder and journal. Only native observations, receipts
// and physical effects are adapted; these tests never post a key to a window.
type cadencePeer struct {
	pendingBackend
	t           *testing.T
	sample, now int64
	after       []int64
	inputs      []InputAction
	outcome     *InputOutcome
	identity    Identity
}

func (p *cadencePeer) ObserveInput(_ context.Context, e bridge.SlotEnvelope, after int64, _ string) (InputObservation, error) {
	p.after = append(p.after, after)
	ready := false
	s := InputObservation{Schema: InputSchema, Bindings: primaryInputBindings(), Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: slotStart(e), GUID: e.GUID, Build: e.Build, SampleMillis: p.sample, InputBlocked: &ready}
	return inputAfterLookup([]memory.Record{inputTestRecord(p.t, s)}, e, after, p.now)
}

func (p *cadencePeer) Input(_ context.Context, a InputAction) (InputOutcome, error) {
	p.inputs = append(p.inputs, a)
	if p.outcome != nil {
		return *p.outcome, nil
	}
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1, AtMillis: p.now}, nil
}

func (p *cadencePeer) Observe(_ context.Context, q ObservationQuery) (Observation, error) {
	if q.Kind != "receipt" {
		return Observation{}, ErrPending
	}
	e := q.Envelope
	i := p.identity
	i.Schema, i.Runtime, i.Owner, i.Fence, i.NextSlot = bridge.SlotSchema, e.Runtime, e.Owner, e.Fence, e.Index+1
	r := Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action}
	switch e.Action {
	case "bind":
		r.State = "bound"
	case "prepare":
		r.State, r.Challenge = "prepared", strings.Repeat("a", 32)
	case "commit":
		r.State = "accepted"
	default:
		return Observation{}, ErrPending
	}
	return Observation{Receipt: r}, nil
}

func (*cadencePeer) Consumed(context.Context, bridge.SlotEnvelope) error { return nil }

func cadenceFixture(t *testing.T) (*Driver, *cadencePeer) {
	t.Helper()
	i := Identity{Schema: IdentitySchema, InputState: InputSchema, Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 500, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.2"}
	p := &cadencePeer{t: t, identity: i, sample: 900, now: 1000}
	d, err := New(filepath.Join(t.TempDir(), "connections", "cadence.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	return d, p
}

func reloadCadenceDriver(t *testing.T, d *Driver, p *cadencePeer) *Driver {
	t.Helper()
	loaded, err := Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	return loaded
}

func seedPreviousCadenceInput(t *testing.T, d *Driver) {
	t.Helper()
	d.State.Input = &InputAttempt{ID: strings.Repeat("2", 32), Exchange: strings.Repeat("3", 32), Runtime: d.State.Identity.Runtime, Kind: "invoke", Outcome: &InputOutcome{Disposition: "submitted", MessagesQueued: 1, AtMillis: 1000}}
	if err := d.Save(context.Background(), "input_observed"); err != nil {
		t.Fatal(err)
	}
}

func TestInputCadenceAcrossExchangeBoundaries(t *testing.T) {
	for _, action := range []string{"prepare", "commit", "confirm", "release", "unbind", "reload"} {
		t.Run(action, func(t *testing.T) {
			ctx := context.Background()
			d, p := cadenceFixture(t)
			seedPreviousCadenceInput(t, d)
			d = reloadCadenceDriver(t, d, p)
			i := d.State.Identity
			e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: action}
			kind := "invoke"
			if action == "reload" {
				kind = "reload"
			}
			p.sample, p.now = 1099, 1100
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, kind, e); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 0 || p.after[0] != 1000 {
				t.Fatalf("+99ms input was authorized: err=%v after=%v inputs=%d", err, p.after, len(p.inputs))
			}
			p.sample = 1100
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, kind, e); err != nil || len(p.inputs) != 1 || p.inputs[0].AfterMillis != 1000 {
				t.Fatalf("+100ms boundary failed: err=%v inputs=%+v", err, p.inputs)
			}
			d = reloadCadenceDriver(t, d, p)
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, kind, e); err != nil || len(p.inputs) != 1 || len(p.after) != 2 {
				t.Fatalf("submitted exchange replayed: err=%v after=%v inputs=%d", err, p.after, len(p.inputs))
			}
		})
	}
}

func TestInputCadenceNormalTransactionsAndResume(t *testing.T) {
	ctx := context.Background()
	d, p := cadenceFixture(t)
	if err := d.Connect(ctx); err != nil || d.State.Transaction != nil || !d.State.Bound || len(p.inputs) != 1 {
		t.Fatalf("bind did not finish: %v", err)
	}
	if err := d.PrepareRequest(ctx, "cadence-original", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	request, ticket, deadline := d.State.Operation.Request, d.State.Operation.Ticket, d.State.Operation.RecoveryBudget.DeadlineMS
	d = reloadCadenceDriver(t, d, p)
	p.sample, p.now = 1099, 1100
	if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 1 || d.State.Transaction.Envelope.Action != "prepare" {
		t.Fatalf("bind-to-prepare cadence lost: %v", err)
	}
	prepareNonce := d.State.Transaction.Envelope.Nonce
	p.sample = 1100
	if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 2 || d.State.Transaction.Envelope.Action != "commit" || p.inputs[1].Envelope.Nonce != prepareNonce {
		t.Fatalf("prepare-to-commit cadence lost: %v", err)
	}
	commitNonce := d.State.Transaction.Envelope.Nonce
	d = reloadCadenceDriver(t, d, p)
	p.sample, p.now = 1199, 1200
	if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 2 || d.State.Transaction.Envelope.Nonce != commitNonce {
		t.Fatalf("resume bypassed +99ms boundary or replaced nonce: %v", err)
	}
	p.sample = 1200
	if err := d.Run(ctx); !errors.Is(err, ErrPending) || len(p.inputs) != 3 || d.State.Transaction != nil || d.State.Operation.Stage != "running" || p.inputs[2].Envelope.Nonce != commitNonce {
		t.Fatalf("+100ms did not complete the original commit: %v", err)
	}
	d = reloadCadenceDriver(t, d, p)
	if err := d.Run(ctx); !errors.Is(err, ErrPending) || len(p.inputs) != 3 || d.State.Operation.Request != request || d.State.Operation.Ticket != ticket || d.State.Operation.RecoveryBudget.DeadlineMS != deadline {
		t.Fatalf("receipt/result continuation replayed input or changed request/deadline: %v", err)
	}
}

func TestInputCadenceSurvivesTimestampFreeNotSent(t *testing.T) {
	ctx := context.Background()
	d, p := cadenceFixture(t)
	seedPreviousCadenceInput(t, d)
	i := d.State.Identity
	e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: "prepare"}
	p.sample, p.now = 1100, 1100
	p.outcome = &InputOutcome{Disposition: "not_sent", Retryable: true, Reason: "fixture_guard_not_ready"}
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrPending) || len(p.inputs) != 1 {
		t.Fatal(err)
	}
	d = reloadCadenceDriver(t, d, p)
	p.outcome, p.sample = nil, 1099
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 1 || p.after[len(p.after)-1] != 1000 {
		t.Fatalf("not_sent discarded prior input watermark: %v after=%v", err, p.after)
	}
	p.sample = 1100
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); err != nil || len(p.inputs) != 2 {
		t.Fatal("known-zero retry failed", err)
	}
}

func TestInputCadenceSameExchangeUnknownNeverReplays(t *testing.T) {
	for _, disposition := range []string{"submitted", "uncertain", "missing"} {
		t.Run(disposition, func(t *testing.T) {
			ctx := context.Background()
			d, p := cadenceFixture(t)
			seedPreviousCadenceInput(t, d)
			if disposition == "missing" {
				d.State.Input.Outcome = nil
			} else {
				d.State.Input.Outcome.Disposition = disposition
			}
			if err := d.Save(ctx, "input_recovery_fixture"); err != nil {
				t.Fatal(err)
			}
			d = reloadCadenceDriver(t, d, p)
			p.sample, p.now = 1200, 1200
			i := d.State.Identity
			e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: "prepare"}
			if err := d.submitInput(ctx, d.State.Input.Exchange, i.Runtime, "invoke", e); err != nil || len(p.inputs) != 0 || len(p.after) != 0 {
				t.Fatal("unknown input replayed", err)
			}
		})
	}
}

func TestInputCadenceFreshRuntimeDoesNotInheritPreviousRuntime(t *testing.T) {
	ctx := context.Background()
	d, p := cadenceFixture(t)
	seedPreviousCadenceInput(t, d)
	current := d.State.Identity
	current.Runtime = strings.Repeat("4", 32)
	p.sample, p.now = 1050, 1100 // Too early for the old runtime, valid for a fresh bind.
	if err := d.RecoverRuntime(ctx, current); err != nil || !d.State.Bound || len(p.inputs) != 1 || p.after[0] != 0 || p.inputs[0].AfterMillis != 0 || p.inputs[0].Envelope.Runtime != current.Runtime {
		t.Fatalf("fresh runtime inherited old cadence or failed binding: %v after=%v inputs=%+v", err, p.after, p.inputs)
	}
	d = reloadCadenceDriver(t, d, p)
	if err := d.finishRecovery(ctx); err != nil || len(p.inputs) != 1 {
		t.Fatal("runtime recovery replayed bind", err)
	}
}

func TestInputCadenceRejectsNegativeJournalWatermark(t *testing.T) {
	d, _ := cadenceFixture(t)
	seedPreviousCadenceInput(t, d)
	d.State.Input.AfterMillis = -1
	if err := d.Save(context.Background(), "input_intent"); err == nil {
		t.Fatal("negative input watermark persisted")
	}
}
