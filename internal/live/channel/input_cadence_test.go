package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
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
	report      bool
	prepared    string
}

func (p *cadencePeer) ObserveInput(_ context.Context, e bridge.SlotEnvelope, after int64, _ string) (InputObservation, error) {
	p.after = append(p.after, after)
	ready := false
	s := InputObservation{Schema: InputSchema, Bindings: primaryInputBindings(), Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: slotStart(e), GUID: e.GUID, Build: e.Build, SampleMillis: p.sample, InputBlocked: &ready}
	return inputAfterLookup([]memory.Record{inputTestRecord(p.t, s)}, e, after, p.now)
}

func (p *cadencePeer) Input(_ context.Context, a InputAction) (InputOutcome, error) {
	p.inputs = append(p.inputs, a)
	if a.Envelope.Action == "prepare" {
		p.prepared = a.Envelope.Nonce
	}
	if p.outcome != nil {
		return *p.outcome, nil
	}
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1, AtMillis: p.now}, nil
}

func (p *cadencePeer) Observe(_ context.Context, q ObservationQuery) (Observation, error) {
	payload := []byte(`{"ok":true,"result":1}`)
	if q.Kind == "result" && p.report {
		return Observation{Receipt: Receipt{ReportBytes: uint32(len(payload)), ReportChecksum: adler32.Checksum(payload)}, Payload: payload}, nil
	}
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
	case "confirm":
		r.State, r.PreparedNonce = "reported", p.prepared
		r.ReportBytes, r.ReportChecksum = uint32(len(payload)), adler32.Checksum(payload)
	case "release":
		r.State = "released"
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

type cadenceCrashPeer struct {
	*cadencePeer
	action string
}

func (p *cadenceCrashPeer) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	out, err := p.cadencePeer.Input(ctx, a)
	if a.Envelope.Action == p.action {
		panic("cadence host loss after input")
	}
	return out, err
}

func expectCadenceCrash(t *testing.T, run func()) {
	t.Helper()
	defer func() {
		if got := recover(); got != "cadence host loss after input" {
			t.Fatalf("expected crash after physical effect, got %v", got)
		}
	}()
	run()
}

func TestInputCadenceCrashAfterEffectRecoversOriginalReceipt(t *testing.T) {
	for _, action := range []string{"bind", "prepare", "commit"} {
		t.Run(action, func(t *testing.T) {
			ctx := context.Background()
			d, p := cadenceFixture(t)
			p.report = true
			d.Backend = &cadenceCrashPeer{cadencePeer: p, action: action}
			if action == "bind" {
				expectCadenceCrash(t, func() { _ = d.Connect(ctx) })
			} else {
				if err := d.Connect(ctx); err != nil {
					t.Fatal(err)
				}
				if err := d.PrepareRequest(ctx, "cadence-crash-original", "return 1", 5, "opaque"); err != nil {
					t.Fatal(err)
				}
				p.sample, p.now = 1100, 1100
				if action == "commit" {
					if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 2 {
						t.Fatal("prepare did not finish before commit", err)
					}
					p.sample, p.now = 1200, 1200
				}
				expectCadenceCrash(t, func() { _ = d.Run(ctx) })
			}
			original := *d.State.Input
			originalEnvelope := d.State.Transaction.Envelope
			id, connectDeadline := d.State.ID, d.State.ConnectBudget.DeadlineMS
			effect, inputs := p.now, len(p.inputs)
			if action != "bind" && original.AfterMillis == 0 {
				t.Fatal("fixture lost the prior known input watermark")
			}
			d = reloadCadenceDriver(t, d, p)
			if d.State.Input.Outcome != nil || d.State.Input.RecoveryAfterMillis != 0 || d.State.Input.Observation.ObservedAtMillis != 0 || d.State.Input.ID != original.ID || d.State.Transaction.Envelope != originalEnvelope {
				t.Fatal("restart did not retain the original timestamp-free intent")
			}
			// The original receipt can recover, but a sample only 50ms after the
			// actual input must not authorize the next exchange. The host read
			// happens later still, giving a conservative, known upper bound.
			p.sample, p.now = effect+50, effect+150
			if action == "bind" {
				if err := d.Connect(ctx); err != nil || len(p.inputs) != inputs {
					t.Fatal("original bind was replayed instead of observing its receipt", err)
				}
				if err := d.PrepareRequest(ctx, "cadence-crash-original", "return 1", 5, "opaque"); err != nil {
					t.Fatal(err)
				}
			}
			op := *d.State.Operation
			deadline := op.RecoveryBudget.DeadlineMS
			if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != inputs || d.State.Input.Outcome != nil || d.State.Input.RecoveryAfterMillis != p.now {
				t.Fatalf("crash recovery accepted early sample or invented outcome: err=%v input=%+v sends=%d", err, d.State.Input, len(p.inputs))
			}
			bound := p.now
			nextEnvelope := d.State.Transaction.Envelope
			observations := len(p.after)
			if err := d.submitInput(ctx, original.Exchange, original.Runtime, original.Kind, originalEnvelope); err != nil || len(p.after) != observations || len(p.inputs) != inputs {
				t.Fatal("recovered time authorized replay of the original input", err)
			}
			d = reloadCadenceDriver(t, d, p)
			p.sample, p.now = bound+99, bound+99
			if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != inputs || d.State.Input.RecoveryAfterMillis != bound || d.State.Transaction.Envelope != nextEnvelope {
				t.Fatalf("+99ms bypassed recovery bound or moved original exchange: %v", err)
			}
			d = reloadCadenceDriver(t, d, p)
			p.sample, p.now = bound+100, bound+100
			if err := d.Run(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != inputs+1 || p.inputs[inputs].Envelope != nextEnvelope || p.inputs[inputs].AfterMillis != bound {
				t.Fatalf("+100ms did not continue original exchange with recovered native revalidation bound: %v", err)
			}
			// Finish the ordinary prepare/commit/confirm/release transitions.
			// Every original nonce is submitted once, including after journal loads.
			for d.State.Operation.Stage != "complete" {
				d = reloadCadenceDriver(t, d, p)
				p.now += 100
				p.sample = p.now
				err := d.Run(ctx)
				if err != nil && !errors.Is(err, ErrInputObservationStale) || len(p.inputs) > 5 {
					t.Fatalf("normal continuation failed: %v sends=%d", err, len(p.inputs))
				}
			}
			seen := map[string]bool{}
			for _, sent := range p.inputs {
				if seen[sent.Envelope.Nonce] {
					t.Fatal("input nonce replayed", sent.Envelope.Nonce)
				}
				seen[sent.Envelope.Nonce] = true
			}
			if len(p.inputs) != 5 || d.State.ID != id || d.State.ConnectBudget.DeadlineMS != connectDeadline || d.State.Operation.ID != op.ID || d.State.Operation.Request != op.Request || d.State.Operation.Ticket != op.Ticket || d.State.Operation.RecoveryBudget.DeadlineMS != deadline {
				t.Fatal("crash recovery changed connection, request, ticket, deadline or effect count")
			}
		})
	}
}

func TestInputCadenceLegacyUnknownTimeUsesCurrentHostObservation(t *testing.T) {
	for _, disposition := range []string{"missing", "submitted", "uncertain"} {
		t.Run(disposition, func(t *testing.T) {
			ctx := context.Background()
			d, p := cadenceFixture(t)
			seedPreviousCadenceInput(t, d)
			d.State.Input.AfterMillis = 800
			if disposition == "missing" {
				d.State.Input.Outcome = nil
			} else {
				d.State.Input.Outcome.Disposition = disposition
				d.State.Input.Outcome.AtMillis = 0
			}
			// Legacy records need not carry any saved observation. Neither its
			// absence nor the nonzero watermark from an older input proves time.
			if err := d.Save(ctx, "legacy_input_fixture"); err != nil {
				t.Fatal(err)
			}
			d = reloadCadenceDriver(t, d, p)
			i := d.State.Identity
			e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: "prepare"}
			p.sample, p.now = 1050, 1100
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 0 || d.State.Input.RecoveryAfterMillis != 1100 {
				t.Fatalf("unknown time reused old watermark: %v input=%+v", err, d.State.Input)
			}
			d = reloadCadenceDriver(t, d, p)
			p.sample, p.now = 1199, 1200
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 0 || d.State.Input.RecoveryAfterMillis != 1100 {
				t.Fatal("resume moved or bypassed the one-time recovery bound", err)
			}
			p.sample = 1200
			if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); err != nil || len(p.inputs) != 1 || p.inputs[0].AfterMillis != 1100 {
				t.Fatal("legacy recovery did not progress at +100ms", err)
			}
		})
	}
}

func TestInputCadenceHostTimeCannotComeFromWireOrJournal(t *testing.T) {
	d, p := cadenceFixture(t)
	i := d.State.Identity
	e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: "prepare"}
	s, err := p.ObserveInput(context.Background(), e, 0, InputSchema)
	if err != nil || s.ObservedAtMillis != p.now {
		t.Fatal("decoder lost current host uptime", err)
	}
	encoded, err := json.Marshal(s)
	if err != nil || strings.Contains(string(encoded), "observedAtMillis") || strings.Contains(string(encoded), "ObservedAtMillis") {
		t.Fatal("host observation time was serialized", err)
	}
	encoded = append(encoded[:len(encoded)-1], []byte(`,"observedAtMillis":42,"ObservedAtMillis":42}`)...)
	var restored InputObservation
	if err := json.Unmarshal(encoded, &restored); err != nil || restored.ObservedAtMillis != 0 {
		t.Fatal("unverified wire or saved observation supplied host time", err)
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

func TestInputCadenceRecoveredBoundSurvivesNotSent(t *testing.T) {
	ctx := context.Background()
	d, p := cadenceFixture(t)
	seedPreviousCadenceInput(t, d)
	d.State.Input.Outcome = nil
	d.State.Input.AfterMillis, d.State.Input.RecoveryAfterMillis = 800, 1000
	if err := d.Save(ctx, "input_time_recovered"); err != nil {
		t.Fatal(err)
	}
	d = reloadCadenceDriver(t, d, p)
	i := d.State.Identity
	e := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: d.State.Owner, Fence: 1, Index: 1, GUID: i.GUID, Build: i.Build, Action: "prepare"}
	p.sample, p.now = 1100, 1100
	p.outcome = &InputOutcome{Disposition: "not_sent", Retryable: true, Reason: "fixture_guard_not_ready"}
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrPending) || d.State.Input.AfterMillis != 1000 {
		t.Fatal("known-zero attempt lost the recovered upper bound", err)
	}
	d = reloadCadenceDriver(t, d, p)
	p.outcome, p.sample = nil, 1099
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 1 {
		t.Fatal("timestamp-free not_sent bypassed the recovered upper bound", err)
	}
	p.sample = 1100
	if err := d.submitInput(ctx, "next-exchange", i.Runtime, "invoke", e); err != nil || len(p.inputs) != 2 || p.inputs[1].AfterMillis != 1000 {
		t.Fatal("known-zero retry did not continue at the recovered boundary", err)
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
	d.State.Input.Outcome = nil
	d.State.Input.RecoveryAfterMillis = 1000
	if err := d.Save(ctx, "input_time_recovered"); err != nil {
		t.Fatal(err)
	}
	d = reloadCadenceDriver(t, d, p)
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
	for _, recovery := range []bool{false, true} {
		d, _ := cadenceFixture(t)
		seedPreviousCadenceInput(t, d)
		if recovery {
			d.State.Input.RecoveryAfterMillis = -1
		} else {
			d.State.Input.AfterMillis = -1
		}
		if err := d.Save(context.Background(), "input_intent"); err == nil {
			t.Fatal("negative input watermark persisted")
		}
	}
}
