//go:build windows && amd64

package channel

import (
	"context"
)

func (p *Project) Reload(ctx context.Context, id, request string, cache bool) (ProjectResult, error) {
	if err := ValidateRequest(request); err != nil {
		return ProjectResult{}, err
	}
	if _, err := p.metadata(id); err != nil {
		return ProjectResult{}, err
	}
	if prior, err := reloadState(p.log(id), request); err != nil {
		return ProjectResult{}, err
	} else if prior != nil && prior.Reload.Phase == "complete" {
		return presentReload(present(&Driver{State: *prior, Log: p.log(id)}), request), nil
	}
	var historic *State
	result, err := p.drive(ctx, id, cache, func(d *Driver) error {
		// Admission can wait behind a different reload. Recheck the whole
		// request history under the lease, not only the latest request.
		prior, err := reloadState(p.log(id), request)
		if err != nil {
			return err
		}
		if prior != nil && prior.Reload.Phase == "complete" {
			historic = prior
			return nil
		}
		if r := d.State.Reload; r != nil && r.Request == request {
			return d.Continue(ctx)
		}
		if !d.State.Bound || d.State.Transaction != nil || d.State.Operation != nil && d.State.Operation.Stage != "complete" || d.State.Reload != nil && d.State.Reload.Phase != "complete" {
			return ErrPending
		}
		if err := d.checkpoint(ctx); err != nil {
			return err
		}
		d.State.Reload = &ReloadAttempt{Request: request, From: d.State.Identity.Runtime, Phase: "intent"}
		if err := d.Save(ctx, "reload_intent"); err != nil {
			return err
		}
		return d.Continue(ctx)
	})
	if historic != nil {
		result = present(&Driver{State: *historic, Log: p.log(id)})
	}
	return presentReload(result, request), err
}

func presentReload(r ProjectResult, request string) ProjectResult {
	r.Operation = ""
	r.OperationState = ""
	r.Report = nil
	r.ReportOrigin = nil
	r.ReportState = "unavailable"
	r.Cleanup = "none"
	r.CleanupMethod = ""
	r.Complete = r.Reload != nil && r.Reload.Request == request && r.Reload.Phase == "complete" && r.Bound
	if r.Complete {
		r.Stage = "connected"
	}
	return r
}

func reloadState(path, key string) (*State, error) {
	return historicalState(path, func(s State) bool { return s.Reload != nil && s.Reload.Request == key })
}
