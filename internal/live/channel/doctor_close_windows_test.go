//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type doctorSlotClosePeer struct{ *closingReloadPeer }

func (p *doctorSlotClosePeer) ObserveRuntimeReplacement(ctx context.Context, old Identity) (*RuntimeReplacementProof, error) {
	if len(p.inputs) == 0 && !p.beforeInput {
		return nil, nil
	}
	return p.retirement.ObserveRuntimeReplacement(ctx, old)
}

func (p *doctorSlotClosePeer) RetireReplacedRuntime(ctx context.Context, envelope bridge.SlotEnvelope, proof *RuntimeReplacementProof) error {
	return p.retirement.RetireReplacedRuntime(ctx, envelope, proof)
}

func TestDoctorSlotCloseReloadsOnceWithoutPublishingSlots(t *testing.T) {
	d, peer := closingReloadFixture(t)
	p := &doctorSlotClosePeer{peer}
	d.Backend = p
	closeBudget, businessBudget := *d.State.CloseBudget, *d.State.Operation.RecoveryBudget
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := d.closeSlotRepairWith(ctx, p, retirementTarget()); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || d.State.Bound || d.State.Reload == nil || d.State.Reload.Request != "doctor-slots-"+d.State.ID || d.State.Reload.Phase != "complete" {
		t.Fatal("slot repair did not close via its durable reload")
	}
	if len(p.inputs) != 1 || p.inputs[0].Kind != "reload" || len(p.publications) != 0 || p.discoveries != 0 {
		t.Fatal("slot repair published a slot, rebound or repeated input")
	}
	if !reflect.DeepEqual(closeBudget, *d.State.CloseBudget) || !reflect.DeepEqual(businessBudget, *d.State.Operation.RecoveryBudget) {
		t.Fatal("slot repair renewed the original close/business budget")
	}
	if d.State.Operation.Stage != "execution_unknown" || present(d).ReportState != "unavailable" || present(d).Complete {
		t.Fatal("slot repair promoted unknown business")
	}
}

func TestDoctorSlotCloseSavedProofNeedsNoInput(t *testing.T) {
	d, peer := closingReloadFixture(t)
	p := &doctorSlotClosePeer{peer}
	d.Backend = p
	d.State.RuntimeEnd = p.retirement.proof
	p.retirement.proof = nil
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := d.closeSlotRepairWith(ctx, p, retirementTarget()); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || len(p.inputs) != 0 || len(p.publications) != 0 || p.retirement.observed != 0 || d.State.Reload != nil {
		t.Fatal("saved proof caused a fresh input or reload")
	}
}

func TestDoctorSlotCloseRetryRetainsReloadIntentAndDeadline(t *testing.T) {
	for _, outcome := range []string{"submitted", "uncertain", "missing"} {
		t.Run(outcome, func(t *testing.T) {
			d, peer := closingReloadFixture(t)
			p := &doctorSlotClosePeer{peer}
			d.Backend = p
			proof := p.retirement.proof
			p.retirement.proof = nil
			ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
			err := d.closeSlotRepairWith(ctx, p, retirementTarget())
			cancel()
			if !errors.Is(err, ErrPending) || len(p.inputs) != 1 || d.State.Reload == nil || d.State.Reload.Phase != "input_attempted" {
				t.Fatal("missing proof did not preserve reload", err)
			}
			budget := *d.State.Reload.RecoveryBudget
			request := d.State.Reload.Request
			if outcome == "missing" {
				d.State.Input.Outcome = nil
			} else {
				d.State.Input.Outcome = &InputOutcome{Disposition: outcome, MessagesQueued: 1}
			}
			if err := d.Save(context.Background(), "lost_outcome_fixture"); err != nil {
				t.Fatal(err)
			}
			d, err = Load(d.Log, p)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel = context.WithTimeout(context.Background(), 50*time.Millisecond)
			err = d.closeSlotRepairWith(ctx, p, retirementTarget())
			cancel()
			if !errors.Is(err, ErrPending) || len(p.inputs) != 1 || d.State.Reload.Request != request || d.State.Reload.RecoveryBudget.StartedAtMS != budget.StartedAtMS || d.State.Reload.RecoveryBudget.DeadlineMS != budget.DeadlineMS {
				t.Fatal("retry replaced intent, deadline or input", err)
			}
			p.retirement.proof = proof
			ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := d.closeSlotRepairWith(ctx, p, retirementTarget()); err != nil {
				t.Fatal(err)
			}
			if !d.State.Closed || len(p.inputs) != 1 || len(p.publications) != 0 || d.State.Reload.Request != request || d.State.Reload.RecoveryBudget.DeadlineMS != budget.DeadlineMS {
				t.Fatal("proof retry repeated input or replaced control")
			}
		})
	}
}

func TestDoctorSlotCloseDoesNotReplaceActiveOrExpiredControl(t *testing.T) {
	for _, kind := range []string{"automatic", "recovery", "expired_explicit"} {
		t.Run(kind, func(t *testing.T) {
			d, peer := closingReloadFixture(t)
			p := &doctorSlotClosePeer{peer}
			d.Backend = p
			switch kind {
			case "automatic":
				d.State.Reload = &ReloadAttempt{Request: "existing-auto", From: d.State.Identity.Runtime, Phase: "intent", Automatic: true, RecoveryBudget: d.State.Operation.RecoveryBudget}
			case "recovery":
				d.State.Recovery = &RuntimeRecovery{From: d.State.Identity, Phase: "binding"}
			case "expired_explicit":
				if err := d.RequestReload(context.Background(), "existing-explicit"); err != nil {
					t.Fatal(err)
				}
				d.State.Reload.RecoveryBudget = NewDurableBudget(time.Now().Add(-time.Hour), time.Minute, false)
				d.State.Reload.RecoveryBudget.Exhausted = true
			}
			before := d.State.Reload
			var budget DurableBudget
			if before != nil {
				budget = *before.RecoveryBudget
			}
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			err := d.closeSlotRepairWith(ctx, p, retirementTarget())
			if kind == "expired_explicit" {
				if !errors.Is(err, ErrBudgetExhausted) {
					t.Fatal("expired reload got a new budget", err)
				}
			} else if !errors.Is(err, ErrPending) {
				t.Fatal("active control was replaced", err)
			}
			if d.State.Reload != before || before != nil && (before.RecoveryBudget.StartedAtMS != budget.StartedAtMS || before.RecoveryBudget.DeadlineMS != budget.DeadlineMS) || len(p.inputs) != 0 || len(p.publications) != 0 {
				t.Fatal("slot repair replaced active control or sent input")
			}
		})
	}
}
