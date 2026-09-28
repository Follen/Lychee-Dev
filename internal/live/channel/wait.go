package channel

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"time"
)

var errProgress = errors.New("live.channel_progress")

// Continue is the only connected scheduler. Every public intent and automatic
// recovery uses the same deadlines, observations and durable action boundaries.
func (d *Driver) Continue(ctx context.Context) error {
	if _, ok := ctx.Deadline(); !ok {
		return errors.New("live.channel_deadline_required")
	}
	for {
		if err := ctx.Err(); err != nil {
			return errors.Join(ErrPending, err)
		}
		d.Waiting = ""
		err := d.next(ctx)
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
		knownBlock := d.Waiting != "" && d.Waiting != "input_observation_unavailable"
		if !knownBlock && d.State.Bound && (d.State.Reload == nil || d.State.Reload.Phase == "complete") {
			candidate, observeErr := d.Backend.RuntimeCandidate(ctx, d.State.Identity)
			if observeErr != nil {
				return observeErr
			}
			if candidate != nil {
				if err = d.RecoverRuntime(ctx, *candidate); err == nil {
					continue
				}
				if !errors.Is(err, ErrPending) {
					return err
				}
			}
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
	quiescent := (d.State.Transaction == nil || d.State.Transaction.Envelope.Action == "unbind") && (d.State.Operation == nil || d.State.Operation.Stage == "complete" || d.State.Operation.Stage == "execution_unknown")
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
		if op.Stage == "prepared" && d.State.Identity.NextSlot > 49 {
			hash := sha256.Sum256([]byte(op.ID))
			request = fmt.Sprintf("capacity-%x", hash[:16])
		}
		if request != "" {
			d.State.Reload = &ReloadAttempt{Request: request, From: d.State.Identity.Runtime, Phase: "intent"}
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
