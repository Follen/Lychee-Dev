//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type activation struct {
	Budget           *DurableBudget `json:"budget,omitempty"`
	Outcome          *InputOutcome  `json:"inputOutcome,omitempty"`
	Schema           string         `json:"schema"`
	Request          string         `json:"request"`
	Selection        TargetRequest  `json:"selection"`
	Phase            string         `json:"phase"`
	PriorRuntime     string         `json:"priorRuntime,omitempty"`
	InputStep        int            `json:"inputStep"`
	PatchTransitions int            `json:"patchTransitions"`
}

func (p *Project) activationPath(id string) string {
	return p.path("connections", id+".activation.json")
}

// Activate performs one journaled, fixed reload for an explicitly selected clean
// installation. It also handles first installation where no memory descriptor
// exists yet. Neither the patch nor installation bytes alone establish readiness.
func (p *Project) Activate(ctx context.Context, request TargetRequest, key string, cache bool) (ProjectResult, error) {
	if err := ValidateRequest(key); err != nil {
		return ProjectResult{}, err
	}
	if request.Installation == "" || request.PID == 0 {
		return ProjectResult{}, errors.New("live.channel_explicit_target_required")
	}
	target, err := live.ResolveClientWindow(ctx, request.Installation, request.PID)
	if err != nil {
		return ProjectResult{}, err
	}
	if err = deploymentGuard(ctx, target); err != nil {
		return ProjectResult{}, err
	}
	hash := sha256.Sum256([]byte(fmt.Sprintf("%s/%d/%d/%s", strings.ToLower(target.Client.Directory), target.Window.ProcessID, target.Window.ProcessStartedAt, key)))
	index := p.path("activations", fmt.Sprintf("%x.json", hash))
	var ref struct {
		ID string `json:"id"`
	}
	if err = readProjectJSON(index, &ref, 4096); err == nil {
		return p.Resume(ctx, ref.ID, cache)
	} else if !errors.Is(err, os.ErrNotExist) {
		return ProjectResult{}, err
	}
	// Ownership arbitration below handles a crash before the convenience index.
	return p.activateTarget(ctx, target, request, key, index, cache)
}

func (p *Project) activateTarget(ctx context.Context, target live.ClientWindow, request TargetRequest, key, index string, cache bool) (ProjectResult, error) {
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	if owner, busy, err := journal.InspectWindowOwner(ctx, parent, resource); err != nil {
		return ProjectResult{}, err
	} else if busy {
		if owner.WorkspaceID != p.workspaceID() {
			return ProjectResult{}, &journal.WindowOccupied{Owner: owner, Foreign: true}
		}
		var a activation
		if err = readProjectJSON(p.activationPath(owner.OperationID), &a, 16384); err != nil || a.Request != key {
			return ProjectResult{}, &journal.WindowOccupied{Owner: owner}
		}
		return p.Resume(ctx, owner.OperationID, cache)
	}
	id, err := token()
	if err != nil {
		return ProjectResult{}, err
	}
	id = "CON-" + id
	hash := sha256.Sum256([]byte(resource + "/" + id))
	owner := journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: p.workspaceID(), Resource: resource, OperationID: id, IntentSHA256: fmt.Sprintf("%x", hash)}
	meta := projectTarget{Schema: "lycheedev.channel-target.v1", Target: target, Owner: owner}
	a := activation{Schema: "lycheedev.channel-activation.v3", Request: key, Selection: request, Phase: "prepared", Budget: NewDurableBudget(time.Now(), DefaultRecoveryBudget, false)}
	err = journal.BeginConnectionWindow(ctx, parent, owner, func() error {
		if err := writeProjectJSON(ctx, p.path("connections", id+".target.json"), meta); err != nil {
			return err
		}
		if err := writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
			return err
		}
		if index != "" {
			return writeProjectJSON(ctx, index, map[string]string{"id": id})
		}
		return nil
	})
	if err != nil {
		return ProjectResult{}, err
	}
	return p.resumeActivation(ctx, id, cache)
}

func (p *Project) activationStatus(id string) (ProjectResult, error) {
	var a activation
	if err := readProjectJSON(p.activationPath(id), &a, 16384); err != nil {
		return ProjectResult{}, err
	}
	if err := a.validate(); err != nil {
		return ProjectResult{}, err
	}
	return p.presentActivation(id, a, time.Now()), nil
}

func (a activation) validate() error {
	if a.Schema != "lycheedev.channel-activation.v3" || ValidateRequest(a.Request) != nil || a.InputStep < 0 || a.InputStep > 6 || a.Budget == nil || a.Budget.validate() != nil {
		return errors.New("live.channel_activation_invalid")
	}
	if a.Outcome != nil && a.Outcome.validate() != nil {
		return errors.New("live.channel_activation_invalid")
	}
	switch a.Phase {
	case "prepared", "input_attempted", "runtime_selected":
	case "cancelled":
		if !a.inputFree() {
			return errors.New("live.channel_activation_invalid")
		}
	default:
		return errors.New("live.channel_activation_invalid")
	}
	return nil
}

func (p *Project) presentActivation(id string, a activation, now time.Time) ProjectResult {
	c := Continuation{Kind: "continue", Session: id, RequestID: a.Request, Goal: "activation"}
	if a.Phase == "cancelled" {
		c.Kind = "completed"
		return ProjectResult{Session: id, Stage: "activation_cancelled", Closed: true, ReportState: "unavailable", Cleanup: "none", Journal: p.activationPath(id), Continuation: c}
	}
	if a.Budget != nil {
		// Projection only: status must not migrate or mutate the stored budget.
		copy := *a.Budget
		remaining, _, err := copy.Observe(now)
		ms := remaining.Milliseconds()
		c.RemainingBudgetMS = &ms
		if err != nil {
			c.Kind = "budget_exhausted"
			kind := "budget_exhausted"
			if errors.Is(err, ErrBudgetClockRollback) {
				kind = "budget_clock_rollback"
			}
			c.Blocker = &Blocker{Kind: kind, Condition: "explicit_budget_decision"}
		}
	}
	return ProjectResult{Session: id, Stage: "activation_" + a.Phase, ReportState: "unavailable", Cleanup: "none", Journal: p.activationPath(id), Continuation: c}
}

// Called only while holding the exact activation driver/owner lease. Reading
// v1 remains compatible; its first drive grants one explicitly marked window.
func (p *Project) observeActivationBudget(ctx context.Context, id string, a *activation, now time.Time) (time.Duration, error) {
	if err := a.validate(); err != nil {
		return 0, err
	}
	remaining, observed, budgetErr := a.Budget.Observe(now)
	if observed {
		if err := writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
			return 0, err
		}
	}
	return remaining, budgetErr
}

func (p *Project) finishActivationBudget(ctx context.Context, id string, a *activation) error {
	flush, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
	defer cancel()
	// Monotonic expiry remains final even when a wall-clock adjustment makes
	// a subsequent Observe appear to have time left. A shorter caller timeout
	// has a different cause and must not exhaust the durable activation goal.
	if errors.Is(context.Cause(ctx), ErrBudgetExhausted) && !a.Budget.Exhausted {
		a.Budget.Exhausted = true
		if err := writeProjectJSON(flush, p.activationPath(id), a); err != nil {
			return err
		}
	}
	_, err := p.observeActivationBudget(flush, id, a, time.Now())
	return err
}

func (p *Project) resumeActivation(ctx context.Context, id string, cache bool) (r ProjectResult, err error) {
	// Ordering is intentional: diagnostic IO must neither consume the final
	// authority-budget observation nor keep another driver out of the window.
	var finalizeBudget, releaseDriver, closeDiagnostics func() error
	defer func() {
		err = errors.Join(err, finishActivationResources(finalizeBudget, releaseDriver, closeDiagnostics))
	}()
	r, err = p.activationStatus(id)
	if err != nil {
		return r, err
	}
	if r.Closed {
		return p.retire(ctx, id, r)
	}
	meta, err := p.metadata(id)
	if err != nil {
		return r, err
	}
	parent := filepath.Join(meta.Target.Client.Directory, "Interface", "AddOns")
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		if errors.Is(err, journal.ErrBusy) {
			b := Blocker{Kind: "active_driver", Consumer: meta.Owner.OperationID, Installation: parent, Condition: "active_driver_released"}
			r.Continuation.Kind, r.Continuation.Blocker = "wait_active_driver", &b
			return r, errors.Join(err, &BlockedError{Blocker: b})
		}
		return r, err
	}
	releaseDriver = func() error {
		if lease != nil {
			return lease.Close()
		}
		return nil
	}
	// A different driver may have finished activation before lease acquisition.
	if d, e := Load(p.log(id), nil); e == nil {
		// A crash may have persisted the selected runtime but not its bind.
		// Resume through the ordinary driver instead of reporting an unbound
		// connection as successful. Release this lease before reacquiring it.
		if e = lease.Close(); e != nil {
			return present(d), e
		}
		lease = nil
		return p.Resume(ctx, id, cache)
	} else if !errors.Is(e, ErrJournalMissing) {
		return r, e
	}
	var a activation
	if err = readProjectJSON(p.activationPath(id), &a, 16384); err != nil {
		return r, err
	}
	if err = a.validate(); err != nil {
		return r, err
	}
	if a.Phase == "cancelled" {
		if err = lease.Close(); err != nil {
			return p.presentActivation(id, a, time.Now()), err
		}
		lease = nil
		return p.retire(ctx, id, p.presentActivation(id, a, time.Now()))
	}
	remaining, err := p.observeActivationBudget(ctx, id, &a, time.Now())
	r = p.presentActivation(id, a, time.Now())
	if err != nil {
		return r, err
	}
	bounded, cancelBudget := context.WithTimeoutCause(ctx, remaining, ErrBudgetExhausted)
	ctx = bounded
	// Persist expiration/rollback even if discovery or input used up the local
	// context. Do not replace the ordinary driver's projection after handoff.
	finalizeBudget = func() error {
		defer cancelBudget()
		if r.Continuation.Goal != "activation" {
			return nil
		}
		budgetErr := p.finishActivationBudget(ctx, id, &a)
		stage := r.Stage
		r = p.presentActivation(id, a, time.Now())
		r.Stage = stage
		return budgetErr
	}
	if a.Phase == "runtime_selected" {
		return r, ErrJournalMissing
	}
	n, err := p.native(ctx, meta.Target, cache)
	if err != nil {
		return r, err
	}
	closeDiagnostics = n.Close
	guard := func(ctx context.Context) error {
		if err := deploymentGuard(ctx, meta.Target); err != nil {
			return err
		}
		owner, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource)
		if err != nil {
			return err
		}
		if !busy || owner != meta.Owner {
			return errors.New("live.channel_ownership_changed")
		}
		return nil
	}
	if err = guard(ctx); err != nil {
		return r, err
	}
	if a.Phase == "prepared" {
		candidates, _, e := n.Discover(ctx, a.Selection.Character, a.Selection.Realm)
		if e != nil {
			return r, e
		}
		if len(candidates) > 0 {
			a.PriorRuntime = candidates[0].Runtime
			candidate := candidates[0]
			if err := inputCapabilityError(candidate.InputState); err != nil {
				return r, err
			}
			if observedInputCapability(candidate.InputState) {
				if candidate.Owner != "" {
					return r, errors.New("live.channel_ownership_changed")
				}
				// Even the explicit fallback command uses the ordinary coordinator
				// when this runtime advertises memory input telemetry. Never silently
				// downgrade a missing/stale sample into blind input.
				d, e := New(p.log(id), n, candidate)
				if e != nil {
					return r, e
				}
				d.State.ID = id
				d.State.Owner = strings.TrimPrefix(id, "CON-")
				d.State.ConnectBudget = a.Budget
				d.ResultDir = p.path("results")
				d.State.Reload = &ReloadAttempt{Request: a.Request, From: candidate.Runtime, Phase: "intent", RecoveryBudget: a.Budget}
				if e = d.Save(ctx, "activation_observed_reload"); e != nil {
					return r, e
				}
				n.Guard = claimGuard(meta.Target, parent, meta.Owner)
				e = d.Continue(ctx)
				return present(d), e
			}
		}
		a.Phase = "input_attempted"
		if err = writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
			return r, err
		}
		n.Guard = guard
		out, inputErr := n.Input(ctx, InputAction{Kind: "reload_fallback"})
		a.Outcome = &out
		if out.Disposition == "not_sent" {
			a.Phase = "prepared"
		}
		flush, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
		err = writeProjectJSON(flush, p.activationPath(id), a)
		cancel()
		if err != nil {
			return r, err
		}
		if out.Disposition == "not_sent" && out.Retryable {
			return r, ErrPending
		}
		if inputErr != nil {
			return r, inputErr
		}
	}
	for {
		candidates, _, e := n.Discover(ctx, a.Selection.Character, a.Selection.Realm)
		if e != nil {
			return r, e
		}
		for _, candidate := range candidates {
			if candidate.Runtime <= a.PriorRuntime || candidate.Owner != "" || candidate.Build != meta.Target.Client.FullBuild || candidate.Product != meta.Target.Client.Product {
				continue
			}
			if candidate.Slots != bridge.SlotCount || candidate.Inventory != nil && *candidate.Inventory != candidate.Slots {
				return r, errors.New("live.channel_client_restart_required")
			}
			d, e := New(p.log(id), n, candidate)
			if e != nil {
				return r, e
			}
			d.State.ID = id
			d.State.Owner = strings.TrimPrefix(id, "CON-")
			d.State.ConnectBudget = a.Budget
			d.ResultDir = p.path("results")
			if e = d.Save(ctx, "activation_runtime_selected"); e != nil {
				return r, e
			}
			a.Phase = "runtime_selected"
			if e = writeProjectJSON(ctx, p.activationPath(id), a); e != nil {
				return present(d), e
			}
			n.Guard = claimGuard(meta.Target, parent, meta.Owner)
			err = d.Continue(ctx)
			return present(d), err
		}
		r.Stage = "activation_waiting_runtime"
		select {
		case <-ctx.Done():
			return r, errors.Join(ErrPending, ctx.Err())
		case <-time.After(250 * time.Millisecond):
		}
	}
}

// A prepared activation has no input intent. A recorded not_sent outcome may
// return it to prepared, but neither an uncertain intent nor legacy progress
// can be treated as proof that no input occurred.
func (a activation) inputFree() bool {
	return a.InputStep == 0 && (a.Outcome == nil || a.Outcome.Disposition == "not_sent" && a.Outcome.MessagesQueued == 0)
}

func (p *Project) disconnectActivation(ctx context.Context, id string, cache bool) (r ProjectResult, err error) {
	r, err = p.activationStatus(id)
	if err != nil {
		return r, err
	}
	if r.Closed {
		return p.retire(ctx, id, r)
	}
	meta, err := p.metadata(id)
	if err != nil {
		return r, err
	}
	parent := filepath.Join(meta.Target.Client.Directory, "Interface", "AddOns")
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		return r, err
	}
	defer func() {
		if lease != nil {
			err = errors.Join(err, lease.Close())
		}
	}()
	// Re-read both authorities under the exact owner/driver lease. Activation
	// may already have handed off to the ordinary connection driver.
	if _, e := Load(p.log(id), nil); e == nil {
		if err = lease.Close(); err != nil {
			return r, err
		}
		lease = nil
		return p.Disconnect(ctx, id, cache)
	} else if !errors.Is(e, ErrJournalMissing) {
		return r, e
	}
	var a activation
	if err = readProjectJSON(p.activationPath(id), &a, 16384); err != nil {
		return r, err
	}
	if err = a.validate(); err != nil {
		return r, err
	}
	r = p.presentActivation(id, a, time.Now())
	if a.Phase != "cancelled" {
		if a.Phase != "prepared" || !a.inputFree() {
			return r, ErrJournalMissing
		}
		a.Phase = "cancelled"
		// Terminal evidence precedes claim retirement; retries can finish that
		// retirement after a crash without any game IO or a business ledger.
		if err = writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
			return r, err
		}
		r = p.presentActivation(id, a, time.Now())
	}
	if err = lease.Close(); err != nil {
		return r, err
	}
	lease = nil
	return p.retire(ctx, id, r)
}

// Each obligation runs even if a prior one fails. This is resource cleanup,
// not an activation transition or an additional budget/scheduler.
func finishActivationResources(finalizeBudget, releaseDriver, closeDiagnostics func() error) error {
	var result error
	for _, finish := range []func() error{finalizeBudget, releaseDriver, closeDiagnostics} {
		if finish != nil {
			result = errors.Join(result, finish())
		}
	}
	return result
}
