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
			return d.continueOrRetire(ctx, d.Backend.(*Native), false)
		}
		if err := d.RequestReload(ctx, request); err != nil {
			return err
		}
		return d.continueOrRetire(ctx, d.Backend.(*Native), false)
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
	r.Complete = r.Reload != nil && r.Reload.Request == request && r.Reload.Phase == "complete" && (r.Bound || r.Closed)
	r.Continuation.Goal = "reload"
	r.Continuation.RequestID = request
	if r.Complete {
		r.Continuation.Kind = "completed"
		r.Continuation.Blocker = nil
		r.Continuation.RemainingBudgetMS = nil
	}
	if r.Complete && !r.Closed {
		r.Stage = "connected"
	}
	return r
}

func reloadState(path, key string) (*State, error) {
	return historicalState(path, func(s State) bool { return s.Reload != nil && s.Reload.Request == key })
}
