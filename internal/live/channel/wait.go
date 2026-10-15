package channel

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/live/memory"
)

var errProgress = errors.New("live.channel_progress")

// A changed publication is an observation gap, not an input failure. During
// an already-submitted reload, the old Lua state may also disappear before the
// replacement is readable. Other layout/protocol/process failures stay errors.
func (d *Driver) pendingObservation(err error) error {
	reloading := d.State.Reload != nil && d.State.Reload.Phase == "input_attempted"
	if errors.Is(err, memory.ErrMailboxPathChanged) || reloading && (errors.Is(err, memory.ErrMailboxStateNull) || errors.Is(err, memory.ErrMailboxStringLayout)) {
		return errors.Join(ErrPending, err)
	}
	return err
}

// A cold discovery can cost seconds. Start its cooldown after completion so
// the ordinary protocol gets time to progress rather than scanning back to
// back. This is scheduling only, never identity or input authority.
func discoveryCooldown(elapsed time.Duration) time.Duration {
	if elapsed < time.Second {
		return time.Second
	}
	if elapsed > 30*time.Second {
		return 30 * time.Second
	}
	return elapsed
}

// Continue is the only connected scheduler. Every public intent and automatic
// recovery uses the same deadlines, observations and durable action boundaries.
func (d *Driver) Continue(ctx context.Context) (result error) {
	if _, ok := ctx.Deadline(); !ok {
		return errors.New("live.channel_deadline_required")
	}
	defer func() {
		ref, _, _ := d.goalBudget()
		if ref != nil && errors.Is(context.Cause(ctx), ErrBudgetExhausted) {
			result = errors.Join(result, ErrBudgetExhausted, d.finishBudget(ctx))
		}
		if errors.Is(result, context.Canceled) || errors.Is(result, context.DeadlineExceeded) {
			result = errors.Join(ErrPending, result)
		}
		result = errors.Join(result, d.recordBlocker(ctx, result))
	}()
	d.Waiting = ""
	if d.State.RuntimeEnd != nil && !d.State.Closed {
		return runtimeRetirementPending()
	}
	bounded, cancel, err := d.budgetContext(ctx)
	if err != nil {
		if errors.Is(err, ErrBudgetExhausted) && d.closeBookkeepingOnly() {
			if err := ctx.Err(); err != nil {
				return err
			}
			// Successful retirement can outlive its bounded journal write.
			// Finish the existing close transition with the caller's context;
			// the exhausted goal grants no further input or cleanup work.
			d.Waiting = ""
			return d.next(ctx)
		}
		return err
	}
	defer cancel()
	ctx = bounded
	// Long discovery belongs to the invocation boundary, never between input
	// samples. A later resume can still observe replacement before any input.
	if d.closingExplicitReload() && d.State.Reload.Phase == "intent" {
		if err := ctx.Err(); err != nil {
			return errors.Join(ErrPending, err)
		}
		closed, err := d.closeReloadReplacement(ctx)
		if closed {
			return err
		}
		err = d.pendingObservation(err)
		if err != nil && !errors.Is(err, ErrPending) && !(ctx.Err() == nil && d.State.RuntimeEnd == nil && errors.Is(err, context.DeadlineExceeded) && optionalObservationTimeout(err)) {
			return err
		}
	}
	var nextDiscovery time.Time
	for {
		if err := ctx.Err(); err != nil {
			return errors.Join(ErrPending, err)
		}
		d.Waiting = ""
		err := d.pendingObservation(d.next(ctx))
		var blocked *BlockedError
		if errors.As(err, &blocked) {
			d.Waiting = blocked.Blocker.Kind
			if errors.Is(err, ErrPublicationPending) {
				d.Waiting = "shared_publication"
			}
			return err
		}
		if errors.Is(err, ErrPublicationPending) {
			d.Waiting = "shared_publication"
		}
		if errors.Is(err, errProgress) {
			continue
		}
		if !errors.Is(err, ErrPending) {
			return err
		}
		// A known input blocker is not runtime loss. Avoid a full discovery loop
		// while waiting for an editor, combat or a physically held modifier.
		knownBlock := knownInputBlocker(d.Waiting)
		if !d.closingExplicitReload() && (!knownBlock || d.State.Closing && d.Waiting != "input_observation_stale") && ctx.Err() == nil && !time.Now().Before(nextDiscovery) {
			discoveryStarted := time.Now()
			candidate, observeErr := d.Backend.RuntimeCandidate(ctx, d.State.Identity)
			nextDiscovery = time.Now().Add(discoveryCooldown(time.Since(discoveryStarted)))
			observeErr = d.pendingObservation(observeErr)
			if observeErr != nil && !errors.Is(observeErr, ErrPending) {
				return observeErr
			}
			if observeErr == nil && candidate != nil {
				if err = d.pendingObservation(d.RecoverRuntime(ctx, *candidate)); err == nil || errors.Is(err, errProgress) {
					continue
				}
				if errors.As(err, &blocked) {
					return err
				}
				if errors.Is(err, ErrExecutionUnknown) && d.State.Reload != nil && d.State.Reload.Phase == "binding" {
					// The control goal may complete while the original business
					// outcome remains unknown. Project them independently.
					continue
				}
				if !errors.Is(err, ErrPending) {
					return err
				}
			}
		}
		if d.Waiting == "prepared_operation_requires_reload" || d.Waiting == "closing_exchange_unconfirmed" {
			return ErrPending
		}
		timer := time.NewTimer(250 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return errors.Join(ErrPending, ctx.Err())
		case <-timer.C:
		}
	}
}

func (d *Driver) closeBookkeepingOnly() bool {
	s := &d.State
	return s.Closing && !s.Bound && s.Transaction == nil &&
		(s.Operation == nil || s.Operation.Stage == "complete" || s.Operation.Stage == "execution_unknown" || s.Operation.Stage == "cancelled") &&
		(s.Reload == nil || s.Reload.Phase == "complete") &&
		(s.Recovery == nil || s.Recovery.Phase == "complete")
}

// Missing mailbox input is not evidence that the old runtime still exists.
// Keep the existing bounded discovery cooldown as its recovery exit.
func knownInputBlocker(reason string) bool {
	switch reason {
	case "", "input_observation_unavailable":
		return false
	default:
		return true
	}
}
func (d *Driver) next(ctx context.Context) error {
	if d.State.Reload != nil && d.State.Reload.Phase != "complete" {
		return d.stepReload(ctx)
	}
	if d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		if err := d.finishRecovery(ctx); err != nil {
			return err
		}
		return errProgress
	}
	closing := d.State.Closing || d.State.Transaction != nil && d.State.Transaction.Envelope.Action == "unbind"
	quiescent := (d.State.Transaction == nil || d.State.Transaction.Envelope.Action == "unbind") && (d.State.Operation == nil || d.State.Operation.Stage == "complete" || d.State.Operation.Stage == "execution_unknown" || d.State.Operation.Stage == "cancelled")
	if closing && quiescent {
		if err := d.Disconnect(ctx); err != nil {
			return err
		}
		d.State.Closing = false
		d.State.Closed = true
		return d.Save(ctx, "connection_closed")
	}
	if !d.State.Bound {
		err := d.Connect(ctx)
		if err == nil && closing {
			return errProgress
		}
		return err
	}
	op := d.State.Operation
	if op == nil {
		return nil
	}
	if d.State.Transaction == nil {
		request := ""
		if op.Stage == "release_ready" && op.CleanupMethod == "reload_required" {
			request = "cleanup-" + op.ID
		}
		if !closing && op.Stage == "prepared" && d.State.Identity.NextSlot > d.State.Identity.Slots-15 {
			hash := sha256.Sum256([]byte(op.ID))
			request = fmt.Sprintf("capacity-%x", hash[:16])
		}
		if request != "" {
			d.State.Reload = &ReloadAttempt{Request: request, From: d.State.Identity.Runtime, Phase: "intent", RecoveryBudget: op.RecoveryBudget, Automatic: true}
			if err := d.Save(ctx, "automatic_reload_intent"); err != nil {
				return err
			}
			return errProgress
		}
	}
	err := d.Run(ctx)
	if err == nil && closing {
		return errProgress
	}
	if errors.Is(err, ErrCleanupReloadRequired) || errors.Is(err, ErrReloadRequired) {
		return errProgress
	}
	return err
}
