package channel

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
)

// One reload step, driven by Continue. The transport reports physical input;
// only a fresh runtime binding can complete reload. No visual signal is authority.
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
	if r.Phase == "input_attempted" {
		candidate, err := d.Backend.RuntimeCandidate(ctx, d.State.Identity)
		if err != nil {
			return err
		}
		if candidate == nil {
			return ErrPending
		}
		if candidate.Inventory != nil && *candidate.Inventory != 64 {
			return errors.New("live.channel_client_restart_required")
		}
		if candidate.Validate() != nil || candidate.Runtime <= r.From || candidate.Owner != "" || candidate.GUID != d.State.Identity.GUID || candidate.Build != d.State.Identity.Build || candidate.Product != d.State.Identity.Product || candidate.Release != d.State.Identity.Release {
			return ErrPending
		}
		d.State.Identity = *candidate
		d.State.Bound = false
		r.Phase = "binding"
		if err = d.Save(ctx, "reload_runtime_candidate"); err != nil {
			return err
		}
		return errProgress
	}
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
	r.Phase = "complete"
	if err := d.Save(ctx, "reload_verified"); err != nil {
		return err
	}
	return errProgress
}
