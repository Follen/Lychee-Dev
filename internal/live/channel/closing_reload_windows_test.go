//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"hash/adler32"
	"reflect"
	"strings"
	"testing"
	"time"
)

type closingReloadPeer struct {
	*stopReconcilePeer
	retirement        *retirementPeer
	beforeInput       bool
	beforeError       error
	preflights        int
	staleOnce         bool
	inputObservations int
}

func (p *closingReloadPeer) closeReloadRuntime(ctx context.Context, d *Driver) (bool, error) {
	if len(p.inputs) == 0 {
		p.preflights++
	}
	if !p.beforeInput && len(p.inputs) == 0 {
		return false, p.beforeError
	}
	return d.closeReplacedRuntime(ctx, p.retirement, retirementTarget())
}
func closingReloadFixture(t *testing.T) (*Driver, *closingReloadPeer) {
	t.Helper()
	d, transport, retirement := retirementFixture(t)
	peer := &closingReloadPeer{stopReconcilePeer: transport, retirement: retirement}
	d.Backend = peer
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	d.State.CloseBudget = NewDurableBudget(time.Now().Add(-time.Hour), time.Minute, false)
	d.State.CloseBudget.Exhausted = true
	if err := d.Save(context.Background(), "expired_close_fixture"); err != nil {
		t.Fatal(err)
	}
	return d, peer
}
func TestExplicitReloadClosesExpiredGoalWithoutRebinding(t *testing.T) {
	d, p := closingReloadFixture(t)
	closeBudget, businessBudget := *d.State.CloseBudget, *d.State.Operation.RecoveryBudget
	tx := d.State.Transaction.Envelope
	if err := d.RequestReload(context.Background(), "new-control"); err != nil {
		t.Fatal(err)
	}
	control := *d.State.Reload.RecoveryBudget
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := d.Continue(ctx); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || d.State.Bound || d.State.Identity.Runtime == p.retirement.proof.Current.Runtime || d.State.Reload == nil || d.State.Reload.Phase != "complete" {
		t.Fatal("reload did not close original runtime without binding")
	}
	if !reflect.DeepEqual(*d.State.CloseBudget, closeBudget) || !reflect.DeepEqual(*d.State.Operation.RecoveryBudget, businessBudget) {
		t.Fatal("renewed original budgets")
	}
	if d.State.Reload.RecoveryBudget.StartedAtMS != control.StartedAtMS || d.State.Reload.RecoveryBudget.DeadlineMS != control.DeadlineMS {
		t.Fatal("changed control budget")
	}
	if len(p.inputs) != 1 || p.inputs[0].Kind != "reload" || len(p.publications) != 0 || p.discoveries != 0 || !reflect.DeepEqual(p.retirement.retired, []bridge.SlotEnvelope{tx}) {
		t.Fatal("replayed transaction or rebound", p.inputs, p.publications)
	}
	if d.State.Operation.Stage != "execution_unknown" || present(d).ReportState != "unavailable" || present(d).Complete {
		t.Fatal("promoted unknown")
	}
	reloaded, err := Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	if !presentReload(present(reloaded), "new-control").Complete || reloaded.State.Reload.From != tx.Runtime {
		t.Fatal("lost reload audit")
	}
}
func TestClosingExplicitReloadResumeKeepsRequestBudgetAndInput(t *testing.T) {
	d, p := closingReloadFixture(t)
	proof := p.retirement.proof
	p.retirement.proof = nil
	if err := d.RequestReload(context.Background(), "same-control"); err != nil {
		t.Fatal(err)
	}
	budget := *d.State.Reload.RecoveryBudget
	// Persist the reload/input first, then exercise a bounded missing-proof
	// wait. Disk scheduling under the full suite is not the behavior under test.
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	err := d.Continue(ctx)
	cancel()
	if !errors.Is(err, ErrPending) || len(p.inputs) != 1 || d.State.Reload.Phase != "input_attempted" {
		t.Fatal("first wait did not preserve original reload", err)
	}
	d, err = Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	p.retirement.proof = proof
	ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err = d.Continue(ctx); err != nil {
		t.Fatal(err)
	}
	if len(p.inputs) != 1 || len(p.publications) != 0 || d.State.Reload.RecoveryBudget.StartedAtMS != budget.StartedAtMS || d.State.Reload.RecoveryBudget.DeadlineMS != budget.DeadlineMS {
		t.Fatal("resume replayed input or reset budget")
	}
}
func TestClosingReloadParentCancelAndAutomaticBudget(t *testing.T) {
	d, p := closingReloadFixture(t)
	if err := d.RequestReload(context.Background(), "cancel-control"); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	cancel()
	if err := d.Continue(ctx); err == nil || len(p.inputs) != 0 || p.retirement.observed != 0 {
		t.Fatal("cancelled parent progressed", err)
	}
	d.State.Reload.Automatic = true
	ref, _, goal := d.goalBudget()
	if goal != "close" || ref != &d.State.CloseBudget || !(*ref).Exhausted {
		t.Fatal("automatic reload borrowed control budget")
	}
}

func TestClosingReloadDoesNotReplaceActiveControlOrRecovery(t *testing.T) {
	for _, recovery := range []bool{false, true} {
		d, _ := closingReloadFixture(t)
		if recovery {
			from := d.State.Identity
			from.Runtime = "00000000000000000000000000000002"
			d.State.Recovery = &RuntimeRecovery{From: from, Phase: "binding"}
		} else {
			d.State.Reload = &ReloadAttempt{Request: "automatic-existing", From: d.State.Identity.Runtime, Phase: "intent", Automatic: true, ReconcilePending: true, RecoveryBudget: d.State.Operation.RecoveryBudget}
		}
		before := d.State
		if err := d.RequestReload(context.Background(), "replacement-control"); !errors.Is(err, ErrPending) {
			t.Fatal("replaced pending control", err)
		}
		if !reflect.DeepEqual(before, d.State) {
			t.Fatal("mutated active control evidence")
		}
	}
}

func TestClosingExplicitReloadAlsoAvoidsBindWithLiveCloseBudget(t *testing.T) {
	d, p := closingReloadFixture(t)
	d.State.CloseBudget = NewDurableBudget(time.Now(), DefaultCloseBudget, false)
	before := *d.State.CloseBudget
	if err := d.RequestReload(context.Background(), "unexpired-control"); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := d.Continue(ctx); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || d.State.Bound || len(p.publications) != 0 || len(p.inputs) != 1 || p.inputs[0].Kind != "reload" || !reflect.DeepEqual(before, *d.State.CloseBudget) {
		t.Fatal("closing reload depended on expiry")
	}
}

func TestClosingReloadIntentAlreadyReplacedNeedsNoInput(t *testing.T) {
	for _, disposition := range []string{"old_input", "unknown", "submitted"} {
		d, p := closingReloadFixture(t)
		p.beforeInput = true
		if err := d.RequestReload(context.Background(), "already-replaced"); err != nil {
			t.Fatal(err)
		}
		d.State.Input = &InputAttempt{ID: strings.Repeat("a", 32), Exchange: "reload:already-replaced", Runtime: d.State.Identity.Runtime, Kind: "reload"}
		if disposition == "old_input" {
			d.State.Input.Exchange = "old-exchange"
			d.State.Input.Kind = "invoke"
		}
		if disposition != "unknown" {
			d.State.Input.Outcome = &InputOutcome{Disposition: "submitted", MessagesQueued: 1}
		}
		if err := d.Save(context.Background(), "input_intent"); err != nil {
			t.Fatal(err)
		}
		oldInput := d.State.Input
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		err := d.Continue(ctx)
		cancel()
		if err != nil || !d.State.Closed || d.State.RuntimeEnd == nil || len(p.inputs) != 0 || len(p.publications) != 0 || !reflect.DeepEqual(oldInput, d.State.Input) {
			t.Fatal("replayed or fabricated input after replacement", err)
		}
	}
}

func TestClosingReloadUnconfirmedReportRetirementFailure(t *testing.T) {
	for _, policy := range []string{"opaque", "observation"} {
		t.Run(policy, func(t *testing.T) {
			d, p := closingReloadFixture(t)
			op := d.State.Operation
			op.Policy = policy
			op.Stage = "confirm_ready"
			op.PreparedNonce = strings.Repeat("3", 32)
			op.Challenge = strings.Repeat("4", 32)
			op.Result = []byte(`{"ok":true,"resourcesReleased":true}`)
			op.ReportBytes = uint32(len(op.Result))
			op.ReportChecksum = adler32.Checksum(op.Result)
			d.State.Transaction.Envelope.Action = "confirm"
			d.State.Transaction.Envelope.Ticket = op.Ticket
			original := d.State.Transaction.Envelope
			candidate := append([]byte(nil), op.Result...)
			if err := d.RequestReload(context.Background(), "close-unconfirmed"); err != nil {
				t.Fatal(err)
			}
			p.retirement.fail = true
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := d.Continue(ctx); err == nil || d.State.RuntimeEnd == nil {
				t.Fatal("missing retirement fault", err)
			}
			var err error
			d, err = Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			p.retirement.fail = false
			if err = d.continueOrRetireWith(ctx, p.retirement, retirementTarget(), true); err != nil {
				t.Fatal(err)
			}
			if !d.State.Closed || d.State.Bound || d.State.Reload.Phase != "complete" || d.State.Operation.Stage != "execution_unknown" || d.State.Operation.Attempt != 1 || string(d.State.Operation.Result) != string(candidate) || present(d).Complete {
				t.Fatal("lost/promoted unconfirmed report")
			}
			if len(p.inputs) != 1 || p.inputs[0].Kind != "reload" || len(p.publications) != 0 || !reflect.DeepEqual(p.retirement.retired, []bridge.SlotEnvelope{original}) {
				t.Fatal("replayed old exchange or bound replacement")
			}
		})
	}
}

func TestClosingReloadIntentObservationTimeoutOrHardFault(t *testing.T) {
	fault := errors.New("target identity failure")
	for _, tc := range []struct {
		name      string
		err       error
		completes bool
	}{
		{"optional_timeout", context.DeadlineExceeded, true},
		{"fault", fault, false},
		{"mixed", errors.Join(context.DeadlineExceeded, fault), false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d, p := closingReloadFixture(t)
			p.beforeError = tc.err
			if err := d.RequestReload(context.Background(), "intent-gate"); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			err := d.Continue(ctx)
			if tc.completes {
				if err != nil || !d.State.Closed || len(p.inputs) != 1 {
					t.Fatal("optional observation prevented authorised reload", err)
				}
			} else if !errors.Is(err, tc.err) || len(p.inputs) != 0 || d.State.RuntimeEnd != nil || d.State.Closed {
				t.Fatal("hard observation fault sent input", err)
			}
		})
	}
}

func (p *closingReloadPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	p.inputObservations++
	if p.staleOnce {
		p.staleOnce = false
		return InputObservation{}, ErrInputObservationStale
	}
	return p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
}

func TestClosingReloadPreflightDoesNotInterruptInputSampling(t *testing.T) {
	d, p := closingReloadFixture(t)
	p.staleOnce = true
	if err := d.RequestReload(context.Background(), "sampling-control"); err != nil {
		t.Fatal(err)
	}
	d.State.Blocker = &Blocker{Kind: "budget_exhausted", Condition: "old close budget"}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := d.continueOrRetireWith(ctx, p.retirement, retirementTarget(), true); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || p.preflights != 1 || p.inputObservations != 2 || len(p.inputs) != 1 || p.inputs[0].Kind != "reload" {
		t.Fatalf("input continuation repeated preflight: preflight=%d observations=%d inputs=%d closed=%v", p.preflights, p.inputObservations, len(p.inputs), d.State.Closed)
	}
}

func TestExpiredClosingReloadStillAllowsReadOnlyRetirement(t *testing.T) {
	for _, phase := range []string{"intent", "input_attempted"} {
		t.Run(phase, func(t *testing.T) {
			d, p := closingReloadFixture(t)
			if err := d.RequestReload(context.Background(), "expired-control"); err != nil {
				t.Fatal(err)
			}
			d.State.Reload.Phase = phase
			d.State.Reload.RecoveryBudget = NewDurableBudget(time.Now().Add(-time.Hour), time.Minute, false)
			d.State.Reload.RecoveryBudget.Exhausted = true
			old := *d.State.Reload.RecoveryBudget
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := d.continueOrRetireWith(ctx, p.retirement, retirementTarget(), true); err != nil {
				t.Fatal(err)
			}
			if !d.State.Closed || len(p.inputs) != 0 || len(p.publications) != 0 || !reflect.DeepEqual(old, *d.State.Reload.RecoveryBudget) {
				t.Fatal("exhausted control prevented pure retirement or renewed input")
			}
		})
	}
}
