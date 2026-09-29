package channel

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"time"
)

var errProgress = errors.New("live.channel_progress")

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
		if err != nil && !(ctx.Err() == nil && d.State.RuntimeEnd == nil && errors.Is(err, context.DeadlineExceeded) && optionalObservationTimeout(err)) {
			return err
		}
	}
	var nextDiscovery time.Time
	for {
		if err := ctx.Err(); err != nil {
			return errors.Join(ErrPending, err)
		}
		d.Waiting = ""
		err := d.next(ctx)
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
			if tx := d.State.Transaction; !d.State.Bound && d.State.Operation == nil && tx != nil && tx.Envelope.Action == "bind" && tx.Phase == "input_attempted" {
				if recoverErr := d.RecoverBinding(ctx); recoverErr == nil {
					nextDiscovery = time.Now().Add(discoveryCooldown(time.Since(discoveryStarted)))
					continue
				} else if !errors.Is(recoverErr, ErrPending) {
					return recoverErr
				}
			}
			candidate, observeErr := d.Backend.RuntimeCandidate(ctx, d.State.Identity)
			nextDiscovery = time.Now().Add(discoveryCooldown(time.Since(discoveryStarted)))
			if observeErr != nil {
				return observeErr
			}
			if candidate != nil {
				if err = d.RecoverRuntime(ctx, *candidate); err == nil {
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

// Missing optical liveness is not evidence that the old runtime still exists.
// Keep the existing bounded discovery cooldown as its recovery exit.
func knownInputBlocker(reason string) bool {
	switch reason {
	case "", "input_observation_unavailable", "input_signal_unavailable", "input_signal_waiting", "input_signal_unknown":
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
		if !closing && op.Stage == "prepared" && d.State.Identity.NextSlot > 49 {
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
	if errors.Is(err, ErrCleanupReloadRequired) {
		return errProgress
	}
	return err
}
