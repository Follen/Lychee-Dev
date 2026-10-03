//go:build windows && amd64

package duplexhost

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Inspect observes a saved connection without creating a lease, claiming,
// binding, renewing, repairing or writing inbox. A verified saved result is
// preserved when the current runtime is unavailable or changed.
func (p *Project) Inspect(ctx context.Context, id string) (ProjectResult, error) {
	meta, e := p.metadata(id)
	if e != nil {
		return ProjectResult{}, e
	}
	var st duplex.State
	if e = boundedJSON(filepath.Join(p.path(id), "state.json"), &st, 4*duplex.MaxSourceBytes); e != nil {
		return ProjectResult{Session: id, Journal: filepath.Join(p.path(id), "state.json")}, e
	}
	if e = duplex.ValidateState(st); e != nil {
		return ProjectResult{Session: id, Journal: filepath.Join(p.path(id), "state.json")}, errors.Join(errors.New("live.duplex_journal_invalid"), e)
	}
	r := p.present(ctx, id, st, nil)
	r.Target = &meta.Target
	r.Diagnostics = map[string]Diagnostic{}
	observed, observeErr := p.inspectWindow(ctx, meta.Target, r)
	if observed.Status != nil && observed.Status.Identity != st.Identity {
		observed.Diagnostics["connectionIdentity"] = Diagnostic{"changed", "original request is retained; no replay into replacement runtime"}
		observeErr = errors.Join(observeErr, duplex.ErrIdentity)
	} else if observed.Status != nil {
		observed.Diagnostics["connectionIdentity"] = Diagnostic{"verified", ""}
	}
	return observed, observeErr
}

// InspectTarget is the pre-connect read-only diagnostic path. Busy business
// readiness is reported independently from cancellation/close capability.
func (p *Project) InspectTarget(ctx context.Context, req TargetRequest) (ProjectResult, error) {
	target, e := live.ResolveClientWindow(ctx, req.Installation, req.PID)
	if e != nil {
		return ProjectResult{Stage: "unknown", Diagnostics: map[string]Diagnostic{"processIdentity": {"unavailable", e.Error()}}}, e
	}
	r := ProjectResult{Stage: "unknown", Target: &target, ReportState: "unavailable", Cleanup: "none", Diagnostics: map[string]Diagnostic{}}
	if req.Build != "" && req.Build != target.Client.FullBuild || req.Product != "" && req.Product != target.Client.Product {
		return r, errors.New("live.duplex_target_mismatch")
	}
	r, e = p.inspectWindow(ctx, target, r)
	if s := r.Status; s != nil && (req.Character != "" && req.Character != s.Character || req.Realm != "" && req.Realm != s.Realm) {
		r.Diagnostics["actorReady"] = Diagnostic{"changed", "requested character or realm differs from observed actor"}
		e = errors.Join(e, duplex.ErrIdentity)
	}
	return r, e
}

func (p *Project) inspectWindow(ctx context.Context, target live.ClientWindow, r ProjectResult) (ProjectResult, error) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	var errs []error
	if e := live.ConfirmClientWindow(ctx, target); e != nil {
		r.Diagnostics["processIdentity"] = Diagnostic{"unavailable", e.Error()}
		return r, e
	}
	r.Diagnostics["processIdentity"] = Diagnostic{"verified", "exact PID, creation instance and executable"}
	if e := deployment(ctx, target); e != nil {
		r.Diagnostics["installed"] = Diagnostic{"unavailable", e.Error()}
		errs = append(errs, e)
	} else {
		r.Diagnostics["installed"] = Diagnostic{"clean", buildinfo.Version}
	}
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	owner, busy, e := journal.InspectWindowOwner(ctx, addonParent(target), resource)
	if e != nil {
		r.Diagnostics["ownerAvailable"] = Diagnostic{"unknown", e.Error()}
		errs = append(errs, e)
	} else if !busy {
		r.Diagnostics["ownerAvailable"] = Diagnostic{"free", ""}
	} else if owner.WorkspaceID == p.workspaceID() {
		r.Diagnostics["ownerAvailable"] = Diagnostic{"own", owner.OperationID}
	} else {
		r.Diagnostics["ownerAvailable"] = Diagnostic{"foreign", owner.OperationID}
	}
	n, e := OpenNative(ctx, target)
	if e != nil {
		r.Diagnostics["runtimePublished"] = Diagnostic{"unavailable", e.Error()}
		return r, errors.Join(append(errs, e)...)
	}
	defer n.Close(ctx)
	profile := n.WriteCapability()
	r.Diagnostics["rootRecipe"] = Diagnostic{"resolved", n.Mailbox.Binding().Evidence.RecipeID}
	state := "unverified"
	if profile.Eligible {
		state = "eligible"
	}
	r.Diagnostics["writerProfile"] = Diagnostic{state, profile.Validation + ": " + profile.LayoutEvidence + "; " + profile.LifetimeEvidence}
	s, e := n.Observe(ctx)
	if e != nil {
		r.Diagnostics["runtimePublished"] = Diagnostic{"unavailable", e.Error()}
		return r, errors.Join(append(errs, e)...)
	}
	r.Status = &s
	if _, e := n.Mailbox.ResolveDuplexArray(ctx, []memory.DuplexPath{{Name: "inbox"}, {Name: "control"}, {Name: "bindResume"}}, s.Runtime, s.Arena, 80+256); e != nil {
		r.Diagnostics["numericLayout"] = Diagnostic{"unavailable", e.Error()}
	} else {
		r.Diagnostics["numericLayout"] = Diagnostic{"observed", "read-only six-number calibration; not write eligibility"}
	}
	r.Diagnostics["runtimePublished"] = Diagnostic{"verified", s.Runtime}
	r.Diagnostics["actorReady"] = Diagnostic{"verified", s.ActorGUID}
	if !s.ActorReady || s.ActorGUID == "" {
		r.Diagnostics["actorReady"] = Diagnostic{"unavailable", "no current ordinary actor GUID"}
	}
	freshCtx, freshCancel := context.WithTimeout(ctx, time.Second)
	defer freshCancel()
	fresh := false
	for {
		timer := time.NewTimer(25 * time.Millisecond)
		select {
		case <-freshCtx.Done():
			timer.Stop()
		case <-timer.C:
		}
		if freshCtx.Err() != nil {
			break
		}
		next, err := n.Observe(freshCtx)
		if err != nil {
			errs = append(errs, err)
			break
		}
		if next.Identity != s.Identity || next.StatusSequence < s.StatusSequence || next.Heartbeat < s.Heartbeat {
			errs = append(errs, duplex.ErrIdentity)
			break
		}
		if next.StatusSequence > s.StatusSequence && next.Heartbeat > s.Heartbeat {
			s = next
			r.Status = &s
			fresh = true
			break
		}
	}
	if fresh {
		r.Diagnostics["runtimeFresh"] = Diagnostic{"advancing", ""}
	} else {
		r.Diagnostics["runtimeFresh"] = Diagnostic{"unknown", "heartbeat did not advance within the bounded observation window"}
	}
	r.Diagnostics["actorReady"] = Diagnostic{"verified", s.ActorGUID}
	if !s.ActorReady || s.ActorGUID == "" {
		r.Diagnostics["actorReady"] = Diagnostic{"unavailable", "current actor or world readiness is unavailable"}
	}
	for name, ready := range map[string]bool{"transportReady": s.TransportReady, "businessReady": s.BusinessReady, "controlReady": s.ControlReady} {
		state := "false"
		if ready {
			state = "true"
		}
		r.Diagnostics[name] = Diagnostic{state, s.Phase}
	}
	if fresh && s.TransportReady && s.ControlReady && profile.Eligible {
		r.Stage = "healthy"
	} else {
		r.Stage = "degraded"
	}
	return r, errors.Join(errs...)
}
