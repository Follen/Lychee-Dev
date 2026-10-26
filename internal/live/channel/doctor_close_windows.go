//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Slot repair cannot publish even an unbind through a damaged pool. Preserve
// the original close intent, then destroy its runtime using the independent
// reload coordinator before maintenance may archive the pool.
func (p *Project) closeForSlotRepair(ctx context.Context, id string) (ProjectResult, error) {
	return p.disconnectMode(ctx, id, true, true)
}

// The caller holds the exact driver lease and has persisted RequestClose.
// Positive runtime proof takes precedence over input, including on retries of
// a reload whose submission outcome was lost.
func (d *Driver) closeSlotRepairWith(ctx context.Context, backend runtimeRetirementBackend, target desktop.WindowIdentity) error {
	if d.State.RuntimeEnd != nil || d.closeBookkeepingOnly() {
		return d.continueOrRetireWith(ctx, backend, target, true)
	}
	probeCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
	closed, err := d.closeReplacedRuntime(probeCtx, backend, target)
	cancel()
	if closed {
		return err
	}
	if err != nil {
		if d.State.RuntimeEnd != nil || ctx.Err() != nil || !errors.Is(err, context.DeadlineExceeded) || !optionalObservationTimeout(err) {
			if d.State.RuntimeEnd == nil && ctx.Err() == nil && errors.Is(err, memory.ErrMailboxPathChanged) {
				return d.pendingObservation(err)
			}
			return err
		}
	}
	if reload := d.State.Reload; reload != nil && reload.Phase != "complete" {
		// Automatic reload/recovery may still require a bind slot. Retain its
		// exact request and deadline; slot repair cannot advance that path.
		if reload.Automatic {
			return ErrPending
		}
		return d.Continue(ctx)
	}
	if d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		return ErrPending
	}
	if err := d.RequestReload(ctx, "doctor-slots-"+d.State.ID); err != nil {
		return err
	}
	return d.Continue(ctx)
}
