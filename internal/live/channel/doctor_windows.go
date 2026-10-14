//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

const ProtocolName = bridge.MailboxSchema

type DoctorCheck struct {
	ID       string `json:"id"`
	State    string `json:"state"`
	Detail   string `json:"detail"`
	NextStep string `json:"nextStep,omitempty"`
}
type BootstrapState struct {
	Schema         string        `json:"schema"`
	Bootstrap      string        `json:"bootstrap"`
	Release        string        `json:"release"`
	Phase          string        `json:"phase"`
	Reason         string        `json:"reason"`
	BindingFailure string        `json:"bindingFailure"`
	Bindings       InputBindings `json:"bindings"`
}
type DoctorDeployment struct {
	State   string                   `json:"state"`
	Version string                   `json:"version,omitempty"`
	Commit  string                   `json:"commit,omitempty"`
	Detail  string                   `json:"detail,omitempty"`
	Slots   *delivery.SlotAssessment `json:"slots,omitempty"`
}
type DoctorResult struct {
	Protocol   string               `json:"protocol"`
	Healthy    bool                 `json:"healthy"`
	Checks     []DoctorCheck        `json:"checks"`
	Target     *live.ClientWindow   `json:"target,omitempty"`
	Deployment *DoctorDeployment    `json:"deployment,omitempty"`
	Bootstrap  *BootstrapState      `json:"bootstrap,omitempty"`
	Identity   *Identity            `json:"identity,omitempty"`
	Input      *InputObservation    `json:"input,omitempty"`
	Owner      *journal.WindowOwner `json:"owner,omitempty"`
	Connection *ProjectResult       `json:"connection,omitempty"`
}

func (r *DoctorResult) add(id, state, detail, next string) {
	r.Checks = append(r.Checks, DoctorCheck{ID: id, State: state, Detail: detail, NextStep: next})
	if state == "blocked" || state == "error" {
		r.Healthy = false
	}
}

// Doctor observes one target without claiming it, loading slots, or sending keys.
// Both command entrypoints call this same implementation.
func (p *Project) Doctor(ctx context.Context, request TargetRequest, session string) (r DoctorResult) {
	r.Protocol, r.Healthy = ProtocolName, true
	var target live.ClientWindow
	var err error
	if session != "" {
		meta, e := p.metadata(session)
		if e != nil {
			r.add("live.target", "error", e.Error(), "use the original project and CON id")
			return
		}
		target = meta.Target
		err = live.ConfirmClientWindow(ctx, target)
		if status, e := p.Status(session); e == nil {
			r.Connection = &status
		}
	} else {
		target, err = live.ResolveClientWindow(ctx, request.Installation, request.PID)
	}
	if err != nil {
		r.add("live.target", "blocked", err.Error(), "select one running WoW PID with live instances")
		return
	}
	r.Target = &target
	r.add("live.target", "ok", fmt.Sprintf("PID %d, %s", target.Window.ProcessID, target.Client.FullBuild), "")
	deployment, err := delivery.InspectAddonDeployment(ctx, target.Client.Directory)
	if err != nil {
		r.add("live.installation", "error", err.Error(), "inspect addon status and the selected installation")
	} else {
		state := deployment.Installation
		r.Deployment = &DoctorDeployment{State: state.State, Detail: state.Detail, Slots: deployment.Slots}
		if state.Receipt != nil {
			r.Deployment.Version, r.Deployment.Commit = state.Receipt.Version, state.Receipt.Commit
		}
		if state.State != "managed" || state.Receipt == nil || state.Receipt.Version != buildinfo.Version {
			r.add("live.installation", "blocked", state.State, "live doctor --fix --release <current distribution> installs or upgrades a clean managed addon")
		} else {
			r.add("live.installation", "ok", "clean managed addon "+state.Receipt.Version, "")
		}
		slots := deployment.Slots
		if slots == nil || slots.Count != bridge.SlotCount || (slots.State != "managed" && slots.State != "pending") {
			r.add("live.slots", "blocked", "500 managed slots are not available", "repair the managed installation before activation")
		} else {
			r.add("live.slots", "ok", fmt.Sprintf("%d slots, %d pending reservations", slots.Count, slots.Pending), "")
		}
	}
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	owner, busy, err := journal.InspectWindowOwner(ctx, parent, resource)
	if err != nil {
		r.add("live.ownership", "error", err.Error(), "inspect the retained connection journal")
		return
	}
	if busy {
		r.Owner = &owner
		if owner.WorkspaceID != p.workspaceID() {
			r.add("live.ownership", "blocked", "selected PID belongs to another project", "resume or disconnect from its owning project")
		} else {
			r.add("live.ownership", "ok", owner.OperationID, "")
			if status, e := p.Status(owner.OperationID); e == nil {
				r.Connection = &status
			} else {
				r.add("live.connection", "error", e.Error(), "restore the original project's recovery files")
			}
		}
	} else {
		r.add("live.ownership", "ok", "unclaimed", "")
	}
	// A diagnostic uses the same runtime root and bounded reader, without trace
	// writes. A snapshot never grants permission to send input.
	n, err := OpenNative(ctx, target.Window, parent, buildinfo.Version)
	if err != nil {
		r.add("live.memory", "error", err.Error(), "verify the running build and its runtime locator")
		return
	}
	defer n.Close()
	n.Guard = func(ctx context.Context) error { return live.ConfirmClientWindow(ctx, target) }
	found, bootErr := n.Find(ctx, memory.Selector{Kind: bridge.MemoryBootstrap}, true)
	if bootErr == nil && len(found.Records) == 1 {
		rec := found.Records[0]
		var boot BootstrapState
		if json.Unmarshal(rec.Payload, &boot) != nil || boot.Schema != InputSchema || boot.Bootstrap != fmt.Sprintf("%x", rec.Header.Runtime) || boot.Release != buildinfo.Version {
			r.add("live.bootstrap", "error", "invalid bootstrap publication", "install the current addon and reload the selected client")
		} else {
			r.Bootstrap = &boot
			if boot.Phase == "failed" {
				r.add("live.bootstrap", "blocked", boot.Reason, "inspect the addon startup failure; the manual reload binding remains available")
			} else {
				r.add("live.bootstrap", "ok", boot.Phase, "")
			}
		}
	} else {
		r.add("live.bootstrap", "warn", "bootstrap diagnostic is not loaded", "install the current addon and reload the selected client")
	}
	candidates, _, err := n.Discover(ctx, request.Character, request.Realm)
	if err != nil {
		r.add("live.runtime", "blocked", err.Error(), "reload the selected client manually after verifying the installation")
		return
	}
	if len(candidates) == 0 {
		r.add("live.runtime", "blocked", "no current runtime publication", "enter the world, enable the addon, then /reload once; resume the same CON if present")
		return
	}
	identity := candidates[0]
	r.Identity = &identity
	if identity.Build != target.Client.FullBuild || identity.Product != target.Client.Product {
		r.add("live.runtime", "error", "runtime and installation identity differ", "select the matching installation and PID")
		return
	}
	canReload, err := activationInputCapability(ctx, n, identity, uptimeMillis())
	if err != nil {
		r.add("live.runtime", "blocked", err.Error(), "retry diagnosis for the selected client")
		return
	}
	if !canReload || identity.Slots != bridge.SlotCount {
		r.add("live.runtime", "blocked", "older runtime requires activation", "reload the selected client manually; old input protocols do not authorize keys")
		return
	}
	if identity.InputState == bridge.PreviousMailboxSchema {
		r.add("live.runtime", "blocked", "loaded runtime needs the protocol-name update", "live doctor --fix reloads once using its verified two-key INPUT")
	} else {
		r.add("live.runtime", "ok", identity.Runtime, "")
	}
	if identity.Inventory == nil || *identity.Inventory != bridge.SlotCount {
		r.add("live.inventory", "blocked", "client has not discovered all 500 slots", "restart the selected client once after installation")
	} else {
		r.add("live.inventory", "ok", "500 discovered slot metadata entries; enabled/loadable state is not checked", "")
	}
	e := bridge.SlotEnvelope{Runtime: identity.Runtime, Owner: identity.Owner, Fence: identity.Fence, Index: identity.NextSlot, GUID: identity.GUID, Build: identity.Build, Action: "reload"}
	sampled, cancel := context.WithTimeout(ctx, 1500*time.Millisecond)
	defer cancel()
	var input InputObservation
	for {
		input, err = n.ObserveInput(sampled, e, 0, identity.InputState)
		if err == nil || (!errors.Is(err, ErrPending) && !errors.Is(err, ErrInputObservationStale)) {
			break
		}
		select {
		case <-sampled.Done():
			err = sampled.Err()
		case <-time.After(25 * time.Millisecond):
			continue
		}
		break
	}
	if err != nil {
		r.add("live.input", "blocked", err.Error(), "wait for a fresh input sample; check bindings and bridge state")
		return
	}
	r.inspectInput(input)
	return
}

func (r *DoctorResult) inspectInput(input InputObservation) {
	r.Input = &input
	if *input.InputBlocked {
		r.add("live.input", "blocked", input.Reason, "resolve the reported input condition, then resume; no Esc is sent")
	} else {
		r.add("live.input", "ok", "execute "+input.Bindings.Wake+", reload "+input.Bindings.Reload, "")
	}
	// The latest callback explains a failure even when current INPUT is ready.
	// It is neither an execution receipt nor authorization to repeat input.
	if attempt := input.InputDiagnostic; attempt != nil {
		switch attempt.Stage {
		case "load_failed", "no_dispatch", "exception", "rejected":
			next := "inspect the latest callback failure and original CON; diagnosis does not replay input"
			if attempt.Reason == "slot_addon_disabled" {
				next = "enable the reported slot in the selected character's addon settings, reload once, then resume the original CON; do not replay submitted input"
			}
			r.add("live.input_attempt", "error", fmt.Sprintf("latest callback %s at slot %d: %s", attempt.Stage, attempt.Slot, attempt.Reason), next)
		}
	}
}
