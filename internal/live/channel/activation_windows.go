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

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type activation struct {
	Outcome          *InputOutcome `json:"inputOutcome,omitempty"`
	Schema           string        `json:"schema"`
	Request          string        `json:"request"`
	Selection        TargetRequest `json:"selection"`
	Phase            string        `json:"phase"`
	PriorRuntime     string        `json:"priorRuntime,omitempty"`
	InputStep        int           `json:"inputStep"`
	PatchTransitions int           `json:"patchTransitions"`
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
	a := activation{Schema: "lycheedev.channel-activation.v1", Request: key, Selection: request, Phase: "prepared"}
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
	if a.Schema != "lycheedev.channel-activation.v1" || ValidateRequest(a.Request) != nil || a.InputStep < 0 || a.InputStep > 6 {
		return ProjectResult{}, errors.New("live.channel_activation_invalid")
	}
	if a.Outcome != nil && a.Outcome.validate() != nil {
		return ProjectResult{}, errors.New("live.channel_activation_invalid")
	}
	switch a.Phase {
	case "prepared", "input_attempted", "runtime_selected":
	default:
		return ProjectResult{}, errors.New("live.channel_activation_invalid")
	}
	return ProjectResult{Session: id, Stage: "activation_" + a.Phase, ReportState: "unavailable", Cleanup: "none", Journal: p.activationPath(id)}, nil
}

func (p *Project) resumeActivation(ctx context.Context, id string, cache bool) (ProjectResult, error) {
	r, err := p.activationStatus(id)
	if err != nil {
		return r, err
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
			_ = lease.Close()
		}
	}()
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
	if a.Phase == "runtime_selected" {
		return r, ErrJournalMissing
	}
	n, err := p.native(meta.Target, cache)
	if err != nil {
		return r, err
	}
	defer n.Close()
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
			if candidate.InputState == "lycheedev.input.v1" {
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
				d.ResultDir = p.path("results")
				d.State.Reload = &ReloadAttempt{Request: a.Request, From: candidate.Runtime, Phase: "intent"}
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
			if candidate.Inventory != nil && *candidate.Inventory != 64 {
				return r, errors.New("live.channel_client_restart_required")
			}
			d, e := New(p.log(id), n, candidate)
			if e != nil {
				return r, e
			}
			d.State.ID = id
			d.State.Owner = strings.TrimPrefix(id, "CON-")
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
