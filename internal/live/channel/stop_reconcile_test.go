package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type stopReconcilePeer struct {
	pendingBackend
	current      Identity
	discoveries  int
	publications []bridge.SlotEnvelope
	inputs       []InputAction
	advancedSlot int
}

func (p *stopReconcilePeer) Publish(_ context.Context, e bridge.SlotEnvelope) error {
	p.publications = append(p.publications, e)
	if e.Action != "bind" && e.Action != "unbind" {
		return ErrPending
	}
	return nil
}
func (p *stopReconcilePeer) Input(_ context.Context, a InputAction) (InputOutcome, error) {
	p.inputs = append(p.inputs, a)
	return InputOutcome{Disposition: "submitted", MessagesQueued: 1}, nil
}
func (p *stopReconcilePeer) Observe(_ context.Context, q ObservationQuery) (Observation, error) {
	e := q.Envelope
	if q.Kind != "receipt" || e.Runtime != p.current.Runtime || (e.Action != "bind" && e.Action != "unbind") {
		return Observation{}, ErrPending
	}
	i := p.current
	i.Owner = e.Owner
	i.Fence = e.Fence
	i.NextSlot = e.Index + 1
	state := "bound"
	if e.Action == "unbind" {
		state = "unbound"
	}
	return Observation{Receipt: Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action, State: state}}, nil
}
func (p *stopReconcilePeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if e.Runtime != p.current.Runtime {
		return InputObservation{}, ErrPending
	}
	s, err := p.pendingBackend.ObserveInput(ctx, e, after, capability)
	if p.advancedSlot == 0 {
		return s, err
	}
	s.NextSlot = p.advancedSlot
	s.SampleMillis = 1000
	b, _ := json.Marshal(s)
	return inputObservation(memory.Record{Header: bridge.MemoryHeader{Kind: bridge.MemoryInputState}, Payload: b}, e, after, 1000)
}
func (p *stopReconcilePeer) RuntimeCandidate(_ context.Context, from Identity) (*Identity, error) {
	p.discoveries++
	if p.current.Runtime == from.Runtime {
		return nil, nil
	}
	i := p.current
	return &i, nil
}
func (*stopReconcilePeer) Consumed(context.Context, bridge.SlotEnvelope) error { return nil }

func stopReconcileFixture(t *testing.T) (*Driver, *stopReconcilePeer) {
	t.Helper()
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 4, Slots: 64, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: "3.0.0"}
	p := &stopReconcilePeer{current: i}
	d, err := New(filepath.Join(t.TempDir(), "connections", "test.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	d.State.Bound = true
	return d, p
}
func stopReconcileContinue(d *Driver) error {
	return stopReconcileContinueFor(d, 350*time.Millisecond)
}
func stopReconcileContinueFor(d *Driver, budget time.Duration) error {
	ctx, cancel := context.WithTimeout(context.Background(), budget)
	defer cancel()
	return d.Continue(ctx)
}
func TestStopBeforePrepareNeverStartsBusiness(t *testing.T) {
	d, p := stopReconcileFixture(t)
	if err := d.PrepareRequest(context.Background(), "stop-unpublished", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	d.State.Closing = true
	if err := d.Save(context.Background(), "disconnect_intent"); err != nil {
		t.Fatal(err)
	}
	err := stopReconcileContinueFor(d, 5*time.Second)
	for _, e := range p.publications {
		if e.Action == "prepare" || e.Action == "commit" {
			t.Fatalf("close published business action %s", e.Action)
		}
	}
	if err != nil || !d.State.Closed {
		t.Fatalf("unstarted operation prevented close: %v %+v", err, d.State)
	}
}

func TestStopBeforePrepareAtCapacityDoesNotReload(t *testing.T) {
	d, p := stopReconcileFixture(t)
	d.State.Identity.NextSlot = 50
	p.current.NextSlot = 50
	if err := d.PrepareRequest(context.Background(), "stop-at-capacity", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	err := stopReconcileContinueFor(d, 5*time.Second)
	for _, a := range p.inputs {
		if a.Kind == "reload" {
			t.Fatal("closing an unpublished request triggered capacity reload")
		}
	}
	if err != nil || !d.State.Closed {
		t.Fatalf("close at capacity did not complete: %v", err)
	}
}

func TestAutomaticCapacityReloadContinuesOriginalRequest(t *testing.T) {
	d, p := stopReconcileFixture(t)
	d.State.Identity.NextSlot = 50
	if err := d.PrepareRequest(context.Background(), "capacity-original", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	originalID, deadline := d.State.Operation.ID, d.State.Operation.RecoveryBudget.DeadlineMS
	p.current.Runtime = strings.Repeat("2", 32)
	p.current.NextSlot = 1
	if err := stopReconcileContinue(d); !errors.Is(err, ErrPending) {
		t.Fatalf("unfinished business returned success: %v", err)
	}
	if d.State.Reload == nil || !d.State.Reload.Automatic || d.State.Reload.Phase != "complete" || d.State.Operation.ID != originalID || d.State.Operation.RecoveryBudget.DeadlineMS != deadline {
		t.Fatal("capacity reload replaced request or budget")
	}
	if d.State.Transaction == nil || d.State.Transaction.Envelope.Action != "prepare" {
		t.Fatal("automatic reload stopped before resuming business")
	}
}
func TestStopPreparedOperationDoesNotCommit(t *testing.T) {
	d, p := stopReconcileFixture(t)
	if err := d.PrepareRequest(context.Background(), "stop-prepared", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage = "commit_ready"
	op.PreparedNonce = strings.Repeat("3", 32)
	op.Challenge = strings.Repeat("4", 32)
	d.State.Closing = true
	if err := d.Save(context.Background(), "disconnect_intent"); err != nil {
		t.Fatal(err)
	}
	err := stopReconcileContinue(d)
	if len(p.publications) != 0 || len(p.inputs) != 0 {
		t.Fatalf("close must not execute prepared work or unrequested reload: publications=%+v inputs=%+v", p.publications, p.inputs)
	}
	if err == nil || d.Waiting != "prepared_operation_requires_reload" {
		t.Fatalf("missing precise v1 stop blocker: err=%v waiting=%q", err, d.Waiting)
	}
}
func TestStopUnknownCommitDoesNotReplayAfterJournalResume(t *testing.T) {
	d, p := stopReconcileFixture(t)
	if err := d.PrepareRequest(context.Background(), "stop-unknown", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage = "commit_ready"
	op.PreparedNonce = strings.Repeat("3", 32)
	op.Challenge = strings.Repeat("4", 32)
	if err := d.begin(context.Background(), "commit", op.Ticket, func(e *bridge.SlotEnvelope) { e.PreparedNonce = op.PreparedNonce; e.Challenge = op.Challenge }); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "published"
	nonce := d.State.Transaction.Envelope.Nonce
	d.State.Closing = true
	if err := d.Save(context.Background(), "disconnect_intent"); err != nil {
		t.Fatal(err)
	}
	for attempt := 0; attempt < 2; attempt++ {
		err := stopReconcileContinue(d)
		if err == nil || d.State.Closed || d.State.Operation.Stage == "complete" {
			t.Fatalf("unknown commit reported complete: %v", err)
		}
		if len(p.publications) != 0 || len(p.inputs) != 0 {
			t.Fatal("close activated an already published, execution-unknown commit")
		}
		d, err = Load(d.Log, p)
		if err != nil {
			t.Fatal(err)
		}
		if d.State.Transaction == nil || d.State.Transaction.Envelope.Nonce != nonce {
			t.Fatal("lost original unknown commit")
		}
	}
}

func TestStopPersistedBusinessExchangeCrashBoundaries(t *testing.T) {
	for _, action := range []string{"prepare", "commit"} {
		for _, phase := range []string{"intent", "published", "input_attempted", "received"} {
			t.Run(action+"/"+phase, func(t *testing.T) {
				d, p := stopReconcileFixture(t)
				if err := d.PrepareRequest(context.Background(), "stop-crash-boundary", "return 1", 5, "opaque"); err != nil {
					t.Fatal(err)
				}
				op := d.State.Operation
				if action == "commit" {
					op.Stage = "commit_ready"
					op.PreparedNonce = strings.Repeat("3", 32)
					op.Challenge = strings.Repeat("4", 32)
				}
				if err := d.begin(context.Background(), action, op.Ticket, func(e *bridge.SlotEnvelope) {
					if action == "prepare" {
						e.Code = op.Code
						e.CodeBytes = len(op.Code)
						e.CodeChecksum = adler32.Checksum([]byte(op.Code))
						e.Budget = op.Budget
					} else {
						e.PreparedNonce = op.PreparedNonce
						e.Challenge = op.Challenge
					}
				}); err != nil {
					t.Fatal(err)
				}
				tx := d.State.Transaction
				tx.Phase = phase
				if phase == "received" {
					i := d.State.Identity
					i.Owner = d.State.Owner
					i.Fence = 1
					i.NextSlot++
					state := "prepared"
					if action == "commit" {
						state = "accepted"
					}
					tx.Receipt = &Receipt{Identity: i, Nonce: tx.Envelope.Nonce, Ticket: op.Ticket, Action: action, State: state, Challenge: strings.Repeat("4", 32)}
				}
				if err := d.RequestClose(context.Background()); err != nil {
					t.Fatal(err)
				}
				for retry := 0; retry < 2; retry++ {
					var err error
					d, err = Load(d.Log, p)
					if err != nil {
						t.Fatal(err)
					}
					err = stopReconcileContinue(d)
					if !errors.Is(err, ErrPending) {
						t.Fatalf("expected exact observation/reload dependency, got %v", err)
					}
					if len(p.publications) != 0 || len(p.inputs) != 0 {
						t.Fatalf("close crossed business input boundary: publications=%+v inputs=%+v", p.publications, p.inputs)
					}
				}
			})
		}
	}
}
func TestContinueReconcilesRuntimeBeforeInitialBindOrReload(t *testing.T) {
	for _, mode := range []string{"initial_bind", "reload_intent"} {
		t.Run(mode, func(t *testing.T) {
			d, p := stopReconcileFixture(t)
			p.current.Runtime = strings.Repeat("2", 32)
			p.current.NextSlot = 1
			if mode == "initial_bind" {
				d.State.Bound = false
			} else if err := d.RequestReload(context.Background(), "runtime-already-changed"); err != nil {
				t.Fatal(err)
			}
			if err := stopReconcileContinueFor(d, 5*time.Second); err != nil {
				t.Fatalf("current runtime not reconciled: %v discoveries=%d", err, p.discoveries)
			}
			if !d.State.Bound || d.State.Identity.Runtime != p.current.Runtime {
				t.Fatal("did not bind current runtime")
			}
			if len(p.inputs) != 1 || p.inputs[0].Kind != "invoke" || p.inputs[0].Envelope.Action != "bind" || p.inputs[0].Envelope.Runtime != p.current.Runtime {
				t.Fatalf("sent stale-runtime input/reload: %+v", p.inputs)
			}
		})
	}
}
func TestContinueReloadAllowsConsumedSlotWithoutReceipt(t *testing.T) {
	d, p := stopReconcileFixture(t)
	if err := d.PrepareRequest(context.Background(), "report-lost", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	if err := d.begin(context.Background(), "confirm", d.State.Operation.Ticket, nil); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "input_attempted"
	p.advancedSlot = d.State.Identity.NextSlot + 1
	if err := d.RequestReload(context.Background(), "reload-lost-receipt"); err != nil {
		t.Fatal(err)
	}
	err := stopReconcileContinue(d)
	if !errors.Is(err, ErrPending) {
		t.Fatalf("expected wait for runtime after submitted reload, got %v", err)
	}
	if len(p.inputs) != 1 || p.inputs[0].Kind != "reload" {
		t.Fatalf("fresh same-owner slot advancement blocked reload: inputs=%+v waiting=%s", p.inputs, d.Waiting)
	}
	if len(p.publications) != 0 {
		t.Fatal("reload republished uncertain business transaction")
	}
}

func TestContinueSecondRuntimeChangePreservesOriginalRecovery(t *testing.T) {
	d, p := stopReconcileFixture(t)
	d.State.Bound = false
	if err := d.begin(context.Background(), "bind", d.State.Owner, nil); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "input_attempted"
	originalRuntime, originalNonce := d.State.Identity.Runtime, d.State.Transaction.Envelope.Nonce
	second := d.State.Identity
	second.Runtime = strings.Repeat("2", 32)
	second.NextSlot = 1
	// Runtime 2 disappears before its fresh bind. Runtime 3 is now observable.
	p.current = second
	p.current.Runtime = strings.Repeat("3", 32)
	if err := d.RecoverRuntime(context.Background(), second); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	bindNonce := d.State.Transaction.Envelope.Nonce
	for retry := 0; retry < 2; retry++ {
		var err error
		d, err = Load(d.Log, p)
		if err != nil {
			t.Fatal(err)
		}
		err = stopReconcileContinue(d)
		var blocked *BlockedError
		if !errors.As(err, &blocked) || blocked.Blocker.Kind != "runtime_changed_during_recovery" || blocked.Blocker.Condition != "original_process_end_or_protocol_recovery" {
			t.Fatalf("second runtime loss not diagnosed: %v", err)
		}
		if len(p.inputs) != 0 || d.State.Recovery.From.Runtime != originalRuntime || d.State.Recovery.Transaction.Envelope.Nonce != originalNonce || d.State.Transaction == nil || d.State.Transaction.Envelope.Nonce != bindNonce {
			t.Fatal("nested recovery discarded original evidence or sent new input")
		}
		if c := d.continuation(); c.Kind != "needs_decision" {
			t.Fatalf("unsafe automatic continuation: %+v", c)
		}
	}
}
