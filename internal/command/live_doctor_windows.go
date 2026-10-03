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
	check.Data = map[string]any{"diagnostics": result.Diagnostics, "target": result.Target, "sendbox": result.Status}
	if err != nil {
		check.OK = false
		check.Status = "error"
		check.Detail = err.Error()
		check.NextStep = "verify the exact client and clean managed addon; enter a character and enable /dev connect"
		return []vault.Check{check}
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
	return checks
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
