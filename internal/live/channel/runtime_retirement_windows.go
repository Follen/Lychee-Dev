//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
)

type runtimeRetirementBackend interface {
	ObserveRuntimeReplacement(context.Context, Identity) (*RuntimeReplacementProof, error)
	RetireReplacedRuntime(context.Context, bridge.SlotEnvelope, *RuntimeReplacementProof) error
}

// This is an input-free cleanup attempt, not a renewal of any business budget.
// The caller holds the exact connection driver lease. Saved proof is an
// irreversible historical fact and remains usable after another reload/crash.
func (d *Driver) closeReplacedRuntime(ctx context.Context, b runtimeRetirementBackend, target desktop.WindowIdentity) (bool, error) {
	if d.State.Closed {
		return true, nil
	}
	proof := d.State.RuntimeEnd
	if proof == nil {
		var err error
		proof, err = b.ObserveRuntimeReplacement(ctx, d.State.Identity)
		if err != nil || proof == nil {
			return false, err
		}
		if proof.ProcessID != target.ProcessID || proof.ProcessStartedAt != target.ProcessStartedAt {
			return false, errors.New("live.channel_runtime_replacement_target_changed")
		}
		if err = retirementProofMatchesState(d.State, proof); err != nil {
			return false, err
		}
		previous := d.State
		d.State.RuntimeEnd = proof
		if err = d.Save(ctx, "runtime_replacement_observed"); err != nil {
			d.State = previous
			return false, err
		}
	}
	if proof.ProcessID != target.ProcessID || proof.ProcessStartedAt != target.ProcessStartedAt {
		return false, errors.New("live.channel_runtime_replacement_target_changed")
	}
	if err := retirementProofMatchesState(d.State, proof); err != nil {
		return false, err
	}
	for _, tx := range []*Transaction{d.State.Transaction, recoveryTransaction(d.State.Recovery)} {
		if tx != nil {
			if err := b.RetireReplacedRuntime(ctx, tx.Envelope, proof); err != nil {
				return false, err
			}
		}
	}
	previous := d.State
	// closeEndedState preserves candidate bytes but does not promote them;
	// only already verified reports become complete through runtime destruction.
	if d.State.Operation != nil {
		op := *d.State.Operation
		d.State.Operation = &op
	}
	var completedReload *ReloadAttempt
	if d.closingExplicitReload() {
		copy := *d.State.Reload
		copy.Phase = "complete"
		completedReload = &copy
	}
	closeEndedState(&d.State)
	if completedReload != nil {
		d.State.Reload = completedReload
	}
	if err := d.Save(ctx, "runtime_replacement_closed"); err != nil {
		d.State = previous
		return false, err
	}
	return true, nil
}

func (n *Native) RetireReplacedRuntime(ctx context.Context, e bridge.SlotEnvelope, proof *RuntimeReplacementProof) (err error) {
	if proof == nil || proof.ProcessID != n.Target.ProcessID || proof.ProcessStartedAt != n.Target.ProcessStartedAt ||
		proof.Current.Runtime == e.Runtime || proof.Current.Build != e.Build || proof.Current.Release != n.Version || n.Guard == nil {
		return errors.New("live.channel_runtime_retirement_invalid")
	}
	if err = n.Guard(ctx); err != nil {
		return err
	}
	if err = n.lockPublication(ctx); err != nil {
		return err
	}
	defer func() { err = errors.Join(err, n.unlockPublication()) }()
	if err = n.Guard(ctx); err != nil {
		return err
	}
	return delivery.RetireSlotRuntime(ctx, n.Parent, n.Version, n.Consumer, e, proof.Current.Runtime)
}

func (n *Native) closeReloadRuntime(ctx context.Context, d *Driver) (bool, error) {
	return d.closeReplacedRuntime(ctx, n, n.Target)
}

func (d *Driver) continueOrRetire(ctx context.Context, n *Native, closing bool) error {
	return d.continueOrRetireWith(ctx, n, n.Target, closing)
}

func (d *Driver) continueOrRetireWith(ctx context.Context, backend runtimeRetirementBackend, target desktop.WindowIdentity, closing bool) error {
	// Establish the invocation scope before the optional thirty-second probe.
	// A fresh Native otherwise mistakes that nested deadline for the global
	// observation deadline and keeps it when ordinary close continues. Existing
	// deadlines/credit remain intact; Continue still applies the durable goal cap.
	if observer, ok := backend.(interface {
		beginObservation(context.Context, Identity) error
	}); ok {
		if err := observer.beginObservation(ctx, d.State.Identity); err != nil {
			return err
		}
	}
	if d.closingExplicitReload() && d.State.RuntimeEnd == nil {
		// Inspect this control's budget, not a blocker retained from an older goal.
		available := true
		if budget := d.State.Reload.RecoveryBudget; budget != nil {
			copy := *budget
			_, _, err := copy.Observe(d.now())
			available = err == nil
		}
		if available {
			return d.Continue(ctx)
		}
	}
	if closing || d.State.RuntimeEnd != nil || d.State.Closing || d.continuation().Kind == "budget_exhausted" {
		probeCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
		closed, err := d.closeReplacedRuntime(probeCtx, backend, target)
		cancel()
		if closed {
			return err
		}
		if err != nil {
			// This optional observation has its own shorter budget. Exhausting
			// it without proof says nothing about the ordinary close protocol.
			// Once proof is durable, only exact retirement may continue.
			optionalTimeout := ctx.Err() == nil && d.State.RuntimeEnd == nil &&
				errors.Is(err, context.DeadlineExceeded) && optionalObservationTimeout(err)
			if !optionalTimeout {
				return err
			}
		}
	}
	return d.Continue(ctx)
}
