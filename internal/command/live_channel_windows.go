//go:build windows && amd64

package command

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/duplexhost"
	"github.com/follenfang/lycheedev/internal/selection"
)

func runChannelCommand(ctx context.Context, route, argument string, opts Options, response *Envelope) (bool, int, error) {
	switch route {
	case "live reset", "live bind", "live reload fallback", "live ack", "live finish", "live hide", "live run", "live abandon", "live probe load":
		return true, 3, errors.New("live.duplex_legacy_retired: enable the installed addon with /dev connect; use a CON connection and duplex execute/resume/cancel/disconnect")
	}
	handled := route == "live connect" || route == "live disconnect" || route == "live cancel" || route == "live execute" || route == "live bugs" || route == "live reload" || route == "live status" || route == "live session" || route == "live resume"
	if strings.HasPrefix(opts.session, "CON-") || strings.HasPrefix(argument, "CON-") {
		switch route {
		case "live connect", "live disconnect", "live cancel", "live execute", "live status", "live session", "live resume", "live reload", "live bugs":
			handled = true
		default:
			if strings.HasPrefix(route, "live ") {
				return true, 2, errors.New("CON connections support execute, bugs, status, session, resume, reload and disconnect; cleanup is automatic")
			}
		}
	}
	if !handled {
		return false, 0, nil
	}
	if route != "live connect" && !strings.HasPrefix(opts.session, "CON-") && !strings.HasPrefix(argument, "CON-") {
		return true, 2, errors.New("live.duplex_connection_id_required")
	}
	project, err := duplexhost.OpenProject(opts.project)
	if err != nil {
		return true, 2, err
	}
	wait := opts.waitSeconds
	if wait == 0 {
		wait = 120
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(wait)*time.Second)
	defer cancel()
	if route != "live status" && route != "live session" {
		doctorOpts := opts
		if doctorOpts.session == "" && strings.HasPrefix(argument, "CON-") {
			doctorOpts.session = argument
		}
		preflightLiveDoctor(ctx, doctorOpts, response)
	}
	var result duplexhost.ProjectResult
	switch route {
	case "live bugs":
		if opts.count < 1 || opts.count > 100 || duplexhost.ValidateRequest(opts.request) != nil {
			return true, 2, errors.New("live bugs requires --request and --count <1-100>")
		}
		code := fmt.Sprintf(`local snapshot, reason = LycheeDevInternal.Diagnostics.SnapshotRecentErrors(%d, "provider_storage")
return {schema="lycheedev.bugs.v1",status=snapshot and "completed" or "unavailable",complete=snapshot and snapshot.complete==true or false,snapshot=snapshot,error=reason}`, opts.count)
		result, err = project.Execute(ctx, opts.session, opts.request, code, 5, !opts.noCache)
	case "live connect":
		if opts.wakeBinding != "" || !opts.region.Empty() {
			return true, 2, errors.New("duplex connections and do not accept capture-area or wake-binding")
		}
		if opts.session != "" {
			if opts.installation != "" || opts.pid != 0 || opts.character != "" || opts.realm != "" || opts.snapshot != "" {
				return true, 2, errors.New("resume the retained CON session without new target selectors")
			}
			result, err = project.Resume(ctx, opts.session, !opts.noCache)
		} else {
			req := duplexhost.TargetRequest{Installation: opts.installation, PID: opts.pid, Character: opts.character, Realm: opts.realm}
			if opts.snapshot != "" {
				root, e := workspaceRoot(opts.home)
				if e != nil {
					return true, 2, e
				}
				pin, e := selection.InspectSelection(ctx, root, opts.snapshot)
				if e != nil {
					return true, 2, e
				}
				if pin.Data == nil {
					return true, 2, errors.New("live snapshot requires a pinned data build")
				}
				req.Build, req.Product = pin.Data.FullBuild, pin.Data.Product
			}
			result, err = project.Connect(ctx, req, !opts.noCache)
		}
	case "live execute":
		if (opts.file == "") == (opts.probe == "") || opts.budgetSeconds < 1 || opts.budgetSeconds > 120 || duplexhost.ValidateRequest(opts.request) != nil {
			return true, 2, errors.New("live execute requires --session CON-... --request <key> --budget-seconds <1-120> and exactly one of --file or --probe")
		}
		var code []byte
		if opts.file != "" {
			code, err = readProbeFile(opts.file)
		} else {
			var root string
			root, err = workspaceRoot(opts.home)
			if err == nil {
				var probe live.ProbePutResult
				probe, err = live.ShowProbe(ctx, root, opts.probe)
				code = probe.Revision.Code
			}
		}
		if err != nil {
			return true, 2, err
		}
		result, err = project.Execute(ctx, opts.session, opts.request, string(code), opts.budgetSeconds, !opts.noCache)
	case "live status", "live session":
		result, err = project.Status(argument)
	case "live resume":
		result, err = project.Resume(ctx, argument, !opts.noCache)
	case "live cancel":
		result, err = project.Cancel(ctx, argument)
	case "live disconnect":
		result, err = project.Disconnect(ctx, argument, !opts.noCache)
	case "live reload":
		result, err = project.Reload(ctx, opts.session, opts.request, !opts.noCache)
	}
	response.Context["project"], response.Context["transport"] = project.Root, "memory-duplex-v1"
	if result.Session != "" {
		response.Result, response.OperationID = result, result.Session
		response.Context["session"], response.Context["stage"] = result.Session, result.Stage
		if result.Operation != "" {
			response.Context["operation"] = result.Operation
		}
	}
	return true, 0, classifyChannelResultError(err)
}
