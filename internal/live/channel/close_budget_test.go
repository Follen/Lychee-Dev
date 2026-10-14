package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// No backend call is needed for terminal bookkeeping. The race test permits
// exactly one successful retirement, committed as the close context expires.
type closeBudgetPeer struct {
	consumed []bridge.SlotEnvelope
	calls    []string
	expire   bool
}

func (p *closeBudgetPeer) unexpected(call string) error {
	p.calls = append(p.calls, call)
	return errors.New("unexpected backend call: " + call)
}
func (p *closeBudgetPeer) Publish(context.Context, bridge.SlotEnvelope) error {
	return p.unexpected("Publish")
}
func (p *closeBudgetPeer) Input(context.Context, InputAction) (InputOutcome, error) {
	return InputOutcome{}, p.unexpected("Input")
}
func (p *closeBudgetPeer) ObserveInput(context.Context, bridge.SlotEnvelope, int64, string) (InputObservation, error) {
	return InputObservation{}, p.unexpected("ObserveInput")
}
func (p *closeBudgetPeer) Observe(context.Context, ObservationQuery) (Observation, error) {
	return Observation{}, p.unexpected("Observe")
}
func (p *closeBudgetPeer) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	return nil, p.unexpected("RuntimeCandidate")
}
func (p *closeBudgetPeer) Supersede(context.Context, bridge.SlotEnvelope, Identity) error {
	return p.unexpected("Supersede")
}
func (p *closeBudgetPeer) Consumed(ctx context.Context, e bridge.SlotEnvelope) error {
	if !p.expire {
		return p.unexpected("Consumed")
	}
	p.expire = false
	p.consumed = append(p.consumed, e)
	<-ctx.Done()
	if !errors.Is(context.Cause(ctx), ErrBudgetExhausted) {
		return p.unexpected("retirement did not reach close deadline")
	}
	return nil
}

func TestCloseBudgetExpiresAfterSuccessfulUnbindRetirement(t *testing.T) {
	base := context.Background()
	d, _, now := budgetDriverFixture(t)
	p := &closeBudgetPeer{expire: true}
	d.Backend = p
	if err := d.RequestClose(base); err != nil {
		t.Fatal(err)
	}
	// Freeze wall time so setup cannot consume this accelerated window. The
	// bounded context's monotonic expiry is the signal, never an arbitrary sleep.
	d.State.CloseBudget = NewDurableBudget(*now, 200*time.Millisecond, false)
	originalBudget := *d.State.CloseBudget
	if err := d.begin(base, "unbind", d.State.Owner, nil); err != nil {
		t.Fatal(err)
	}
	e := d.State.Transaction.Envelope
	i := d.State.Identity
	i.Schema, i.NextSlot, i.Owner, i.Fence = e.Schema, e.Index+1, e.Owner, e.Fence
	d.State.Transaction.Receipt = &Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action, State: "unbound"}
	d.State.Transaction.Phase = "received"
	if err := d.Save(base, "verified_unbind_fixture"); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(base, 5*time.Second)
	defer cancel()
	if err := d.Continue(ctx); !errors.Is(err, ErrBudgetExhausted) || len(p.consumed) != 1 || ctx.Err() != nil {
		t.Fatalf("missed successful retirement/deadline boundary: err=%v consumed=%v caller=%v", err, p.consumed, ctx.Err())
	}
	loaded, err := Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	d = loaded
	d.Now = func() time.Time { return *now }
	if d.State.Bound || d.State.Transaction != nil || !d.State.Closing || d.State.Closed || !d.State.CloseBudget.Exhausted {
		t.Fatalf("missing durable unbound boundary: %+v", d.State)
	}
	originalBudget.Exhausted = true
	if *d.State.CloseBudget != originalBudget {
		t.Fatalf("close budget replaced: got=%+v want=%+v", d.State.CloseBudget, originalBudget)
	}
	before := closeBudgetEvidence(t, d.State)
	if err := d.Continue(ctx); err != nil {
		t.Fatalf("already-unbound close cannot finish terminal bookkeeping: %v", err)
	}
	d, err = Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || d.State.Closing || d.State.Blocker != nil || d.continuation().Kind != "completed" {
		t.Fatalf("close did not complete durably: %+v", d.State)
	}
	if closeBudgetEvidence(t, d.State) != before || len(p.calls) != 0 || len(p.consumed) != 1 || p.consumed[0] != e {
		t.Fatalf("terminal close changed evidence or repeated backend work: state=%+v peer=%+v", d.State, p)
	}
}

// Compare all durable evidence except the exact terminal flags, their progress
// counter and the obsolete blocker. Operation/report/input bytes remain exact.
func closeBudgetEvidence(t *testing.T, s State) string {
	t.Helper()
	s.Closed, s.Closing, s.ProgressVersion, s.Blocker = false, false, 0, nil
	b, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func closeBudgetOperation(t *testing.T, d *Driver, stage string) {
	t.Helper()
	if err := d.PrepareRequest(context.Background(), "original-close-request", "counter = counter + 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage, op.PreparedNonce, op.Challenge = stage, strings.Repeat("3", 32), strings.Repeat("4", 32)
	if stage == "confirm_ready" || stage == "result_verified" || stage == "release_ready" || stage == "complete" {
		op.Result = []byte("{\"ok\":true, \"result\":\"<exact>\"}\r\n")
		op.ReportBytes, op.ReportChecksum = uint32(len(op.Result)), adler32.Checksum(op.Result)
	}
	if stage == "complete" {
		op.CleanupMethod = "released"
	}
}

func TestExpiredCloseSnapshotRequiresQuiescentState(t *testing.T) {
	type testCase struct {
		name    string
		mutate  func(*testing.T, *Driver)
		allowed bool
	}
	cases := []testCase{
		{"unbound", nil, true},
		{"still_bound", func(_ *testing.T, d *Driver) { d.State.Bound = true }, false},
		{"completed_reload", func(_ *testing.T, d *Driver) {
			d.State.Reload = &ReloadAttempt{Request: "original-reload", From: d.State.Identity.Runtime, Phase: "complete", RecoveryBudget: d.State.CloseBudget}
		}, true},
		{"completed_recovery", func(_ *testing.T, d *Driver) {
			from := d.State.Identity
			d.State.Identity.Runtime = strings.Repeat("2", 32)
			d.State.Recovery = &RuntimeRecovery{From: from, Phase: "complete"}
		}, true},
		{"runtime_retirement_pending", func(_ *testing.T, d *Driver) {
			old, proof := replacementFixture()
			d.State.Identity, d.State.RuntimeEnd = old, &proof
		}, false},
	}
	for _, stage := range []string{"complete", "cancelled", "execution_unknown", "prepared", "commit_ready", "running", "confirm_ready", "result_verified", "release_ready"} {
		cases = append(cases, testCase{"operation_" + stage, func(t *testing.T, d *Driver) {
			d.State.Bound = true
			closeBudgetOperation(t, d, stage)
			d.State.Bound = false
		}, stage == "complete" || stage == "cancelled" || stage == "execution_unknown"})
	}
	for _, action := range []string{"bind", "unbind"} {
		cases = append(cases, testCase{"pending_" + action, func(t *testing.T, d *Driver) {
			if err := d.begin(context.Background(), action, d.State.Owner, nil); err != nil {
				t.Fatal(err)
			}
			d.State.Transaction.Phase = "input_attempted"
		}, false})
	}
	for _, phase := range []string{"intent", "input_attempted", "binding"} {
		for _, automatic := range []bool{false, true} {
			name := "explicit_reload_"
			if automatic {
				name = "automatic_reload_"
			}
			cases = append(cases, testCase{name + phase, func(_ *testing.T, d *Driver) {
				d.State.Reload = &ReloadAttempt{Request: "original-reload", From: d.State.Identity.Runtime, Phase: phase, Automatic: automatic, RecoveryBudget: d.State.CloseBudget}
			}, false})
		}
	}
	for _, phase := range []string{"binding", "bound"} {
		cases = append(cases, testCase{"recovery_" + phase, func(_ *testing.T, d *Driver) {
			from := d.State.Identity
			d.State.Identity.Runtime = strings.Repeat("2", 32)
			d.State.Recovery = &RuntimeRecovery{From: from, Phase: phase}
		}, false})
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			d, _, now := budgetDriverFixture(t)
			p := &closeBudgetPeer{}
			d.Backend, d.State.Bound = p, false
			d.State.CloseBudget = NewDurableBudget(*now, time.Second, false)
			d.State.CloseBudget.Exhausted = true
			if tc.mutate != nil {
				tc.mutate(t, d)
			}
			// Write the already-existing boundary directly: RequestClose and
			// finishTransaction are not involved in creating this old snapshot.
			d.State.Closing = true
			d.State.Blocker = &Blocker{Kind: "budget_exhausted", Condition: "explicit_budget_decision"}
			if err := d.Save(context.Background(), "expired_close_fixture"); err != nil {
				t.Fatal(err)
			}
			loaded, err := Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			d = loaded
			d.Now = func() time.Time { return *now }
			before := closeBudgetEvidence(t, d.State)
			err = stopReconcileContinueFor(d, 5*time.Second)
			if tc.allowed {
				if err != nil {
					t.Fatalf("quiescent snapshot could not close: %v", err)
				}
			} else if tc.name == "runtime_retirement_pending" {
				var blocked *BlockedError
				if !errors.As(err, &blocked) || blocked.Blocker.Kind != "runtime_retirement_pending" {
					t.Fatalf("lost precise retirement dependency: %v", err)
				}
			} else if !errors.Is(err, ErrBudgetExhausted) {
				t.Fatalf("unfinished close bypassed its deadline: %v", err)
			}
			d, err = Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			if d.State.Closed != tc.allowed || d.State.Closing == tc.allowed || closeBudgetEvidence(t, d.State) != before || len(p.calls) != 0 || len(p.consumed) != 0 {
				t.Fatalf("close changed pending evidence or performed backend work: state=%+v peer=%+v", d.State, p)
			}
		})
	}
}

func TestExpiredQuiescentCloseHonorsCallerCancellation(t *testing.T) {
	d, _, now := budgetDriverFixture(t)
	p := &closeBudgetPeer{}
	d.Backend, d.State.Bound = p, false
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	d.State.CloseBudget = NewDurableBudget(*now, time.Second, false)
	d.State.CloseBudget.Exhausted = true
	if err := d.Save(context.Background(), "expired_close_fixture"); err != nil {
		t.Fatal(err)
	}
	before := closeBudgetEvidence(t, d.State)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	cancel()
	if err := d.Continue(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled caller permitted terminalization: %v", err)
	}
	d, err := Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	if d.State.Closed || !d.State.Closing || closeBudgetEvidence(t, d.State) != before || len(p.calls) != 0 {
		t.Fatalf("cancelled close changed evidence: state=%+v peer=%+v", d.State, p)
	}
}
