//go:build windows && amd64

package command

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/live/duplexhost"
	"github.com/follenfang/lycheedev/internal/vault"
)

func liveDoctorChecks(ctx context.Context, opts Options) []vault.Check {
	if opts.session == "" && opts.installation == "" && opts.pid == 0 {
		return nil
	}
	p, err := duplexhost.OpenProject(opts.project)
	var result duplexhost.ProjectResult
	if err == nil {
		if opts.session != "" {
			if opts.installation != "" || opts.pid != 0 {
				err = errors.New("doctor accepts either --session or target selectors")
			} else {
				result, err = p.Inspect(ctx, opts.session)
			}
		} else {
			result, err = p.InspectTarget(ctx, duplexhost.TargetRequest{Installation: opts.installation, PID: opts.pid})
		}
	}
	return liveDoctorProjection(result, err)
}

func liveDoctorProjection(result duplexhost.ProjectResult, err error) []vault.Check {
	check := vault.Check{ID: "live.duplex", OK: true, Status: "ok"}
	check.Data = map[string]any{"diagnostics": result.Diagnostics, "target": result.Target, "sendbox": result.Status, "nativeReload": result.NativeReload}
	if err != nil {
		check.OK = false
		check.Status = "error"
		check.Detail = err.Error()
		check.NextStep = "verify the exact client and clean managed addon; enter a character and enable /dev connect"
		return append([]vault.Check{check}, lifecycleDoctorChecks(result)...)
	}
	if result.Status == nil {
		check.OK = false
		check.Status = "unknown"
		check.Detail = "no fresh sendbox observation"
	} else {
		s := result.Status
		check.Detail = "phase=" + s.Phase
		if s.Ready {
			check.Detail += "; ready=true"
		} else {
			check.Status = "warn"
			check.Detail += "; ready=false"
		}
		if s.ControlReady {
			check.Detail += "; controls ready"
		} else {
			check.Detail += "; controls unavailable"
		}
	}
	checks := []vault.Check{check}
	if profile, ok := result.Diagnostics["writerProfile"]; ok && profile.State != "eligible" {
		checks = append(checks, vault.Check{ID: "live.duplex.writer", OK: false, Status: "error",
			Code: "live.duplex_writer_profile_unverified", Detail: profile.Reason,
			NextStep: "this exact executable/build has no validated writer profile; native writes remain unavailable"})
	}
	return append(checks, lifecycleDoctorChecks(result)...)
}

// These independent native observations survive an absent or obsolete addon.
// A world warning blocks new business, while controls use their own authority.
func lifecycleDoctorChecks(result duplexhost.ProjectResult) []vault.Check {
	o := result.NativeReload
	if o == nil {
		return nil
	}
	reload := vault.Check{ID: "live.duplex.reload", Status: "error", Detail: o.State, Data: map[string]any{"observation": o}}
	switch o.State {
	case "no_reload_observed":
		reload.OK, reload.Status = true, "ok"
	case "requested", "reloading":
		reload.Status, reload.Code = "warn", "live.duplex_runtime_reloading"
		reload.NextStep = "wait for reload to finish, then observe the exact process again"
	default:
		reload.Code = "live.duplex_reload_state_unavailable"
		reload.NextStep = "resolve and verify the native lifecycle recipe for this exact executable; do not write"
	}
	world := vault.Check{ID: "live.duplex.world", Status: "error", Detail: o.WorldState}
	if o.WorldReady != nil && o.WorldState == "world_ready" && *o.WorldReady {
		world.OK, world.Status = true, "ok"
	} else if o.WorldReady != nil && o.WorldState == "not_in_world" && !*o.WorldReady {
		world.Status, world.Code = "warn", "live.duplex_character_not_in_world"
		world.NextStep = "enter the selected character before new business; cancellation and cleanup have separate readiness"
	} else {
		world.Code = "live.duplex_world_state_unavailable"
		world.NextStep = "verify the IsPlayerInWorld getter recipe; missing evidence must stay unknown"
	}
	return []vault.Check{reload, world}
}

// Every live drive runs doctor and records its findings. Readiness does not
// block cleanup: the coordinator checks exact operation-specific authority.
func preflightLiveDoctor(ctx context.Context, opts Options, response *Envelope) {
	doctor := Envelope{Context: map[string]any{}}
	opts.offline = true
	_, err := runDoctor(ctx, opts, &doctor)
	response.Context["doctor"] = doctor.Result
	if err != nil {
		response.Context["doctorError"] = err.Error()
	}
}
