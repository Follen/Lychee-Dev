package channel

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
)

// RequestReload records an explicit recovery request without discarding an
// uncertain exchange. Open connections bind a fresh runtime; closing connections
// retire exact old claims through positive runtime-destruction evidence.
func (d *Driver) RequestReload(ctx context.Context, request string) error {
	if d.State.RuntimeEnd != nil {
		return runtimeRetirementPending()
	}
	if err := ValidateRequest(request); err != nil {
		return err
	}
	if !d.State.Bound || d.State.Closed || d.State.Reload != nil && d.State.Reload.Phase != "complete" || d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		return ErrPending
	}
	pending := d.State.Transaction != nil || d.State.Operation != nil && d.State.Operation.Stage != "complete"
	if !pending {
		if err := d.checkpoint(ctx); err != nil {
			return err
		}
	}
	d.State.Reload = &ReloadAttempt{Request: request, From: d.State.Identity.Runtime, Phase: "intent", ReconcilePending: pending, RecoveryBudget: NewDurableBudget(d.now(), DefaultRecoveryBudget, false)}
	return d.Save(ctx, "reload_intent")
}

// One reload step, driven by Continue. The transport reports physical input;
// open connections require fresh binding, while closing requires destruction
// proof and retirement. No visual signal is authority.
func (d *Driver) closingExplicitReload() bool {
	r := d.State.Reload
	return d.State.Closing && r != nil && !r.Automatic && r.Phase != "complete"
}

func (d *Driver) stepReload(ctx context.Context) error {
	r := d.State.Reload
	if r.Phase == "intent" {
		exchange := "reload:" + r.Request
		i := d.State.Identity
		envelope := bridge.SlotEnvelope{Runtime: i.Runtime, Owner: i.Owner, Fence: i.Fence, GUID: i.GUID, Build: i.Build, Index: i.NextSlot, Action: "reload"}
		if err := d.submitInput(ctx, exchange, r.From, "reload", envelope); err != nil {
			return err
		}
		r.Phase = "input_attempted"
		if err := d.Save(ctx, "reload_input_attempted"); err != nil {
			return err
		}
		return errProgress
	}
	if d.closingExplicitReload() {
		// Closing never binds the replacement runtime. Its positive destruction
		// proof is enough to retire the original claims without replaying business.
		closed, err := d.closeReloadReplacement(ctx)
		if err != nil {
			return err
		}
		if closed {
			return nil
		}
		return ErrPending
	}
	if r.Phase == "input_attempted" {
		// Runtime discovery/selection belongs to Continue for every goal.
		// A second scanner here would bypass its bounded discovery cadence.
		return ErrPending
	}
	if r.ReconcilePending {
		if err := d.finishRecovery(ctx); err != nil && !errors.Is(err, ErrExecutionUnknown) {
			return err
		}
	} else {
		if err := d.Connect(ctx); err != nil {
			return err
		}
		if op := d.State.Operation; op != nil && op.Stage == "prepared" && op.PreparedNonce == "" {
			origin := d.State.Identity
			op.Origin = &origin
		}
		if op := d.State.Operation; op != nil && op.Stage == "release_ready" && op.CleanupMethod == "reload_required" {
			op.Stage = "complete"
			op.CleanupMethod = "runtime_destroyed"
		}
	}
	r.Phase = "complete"
	if err := d.Save(ctx, "reload_verified"); err != nil {
		return err
	}
	if r.ReconcilePending && !r.Automatic && !d.State.Closing {
		return nil
	}
	return errProgress
}

func (d *Driver) closeReloadReplacement(ctx context.Context) (bool, error) {
	closer, ok := d.Backend.(interface {
		closeReloadRuntime(context.Context, *Driver) (bool, error)
	})
	if !ok {
		return false, nil
	}
	return closer.closeReloadRuntime(ctx, d)
}
