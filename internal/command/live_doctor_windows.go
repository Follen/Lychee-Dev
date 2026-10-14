//go:build windows && amd64

package command

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/vault"
)

func doctorTarget(opts Options) channel.TargetRequest {
	return channel.TargetRequest{Installation: opts.installation, PID: opts.pid}
}
func runLiveDoctorCommand(ctx context.Context, opts Options, response *Envelope) (bool, int, error) {
	if opts.session != "" && (opts.installation != "" || opts.pid != 0) {
		return true, 2, errors.New("use --session with its original --project, or select --installation/--pid")
	}
	if !opts.fix && (opts.release != "" || opts.output != "" || opts.request != "") {
		return true, 2, errors.New("--release, --output and --request require --fix")
	}
	project, err := channel.OpenProject(opts.project)
	if err != nil {
		return true, 2, err
	}
	wait := opts.waitSeconds
	if wait == 0 {
		wait = 30
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(wait)*time.Second)
	defer cancel()
	report := project.Doctor(ctx, doctorTarget(opts), opts.session)
	response.Result = report
	response.Context["project"] = project.Root
	response.Context["protocol"] = channel.ProtocolName
	if !opts.fix {
		if !report.Healthy {
			return true, 0, errDoctorUnhealthy
		}
		return true, 0, nil
	}
	result, err := fixLiveDoctor(ctx, project, opts, report)
	if result.Session != "" {
		report.Connection = &result
		response.OperationID = result.Session
	}
	if err == nil {
		report = project.Doctor(ctx, doctorTarget(opts), result.Session)
		if result.Session != "" {
			report.Connection = &result
		}
	} else {
		report.Healthy = false
		next := "resolve the repair error, then run live doctor --fix for the same target"
		if result.Session != "" {
			next = "live resume " + result.Session + " --project <original-project>"
		}
		report.Checks = append(report.Checks, channel.DoctorCheck{ID: "live.repair", State: "blocked", Detail: err.Error(), NextStep: next})
	}
	response.Result = report
	if err != nil {
		return true, 0, classifyChannelResultError(err)
	}
	if !report.Healthy {
		return true, 0, errDoctorUnhealthy
	}
	return true, 0, nil
}
func fixLiveDoctor(ctx context.Context, project *channel.Project, opts Options, report channel.DoctorResult) (channel.ProjectResult, error) {
	if report.Target == nil {
		return channel.ProjectResult{}, errDoctorUnhealthy
	}
	target := report.Target
	deployment := report.Deployment
	if deployment == nil {
		return channel.ProjectResult{}, errDoctorUnhealthy
	}
	needsInstall := deployment.State != "managed" || deployment.Version != buildinfo.Version || deployment.Slots == nil || deployment.Slots.Count != bridge.SlotCount || (deployment.Slots.State != "managed" && deployment.Slots.State != "pending")
	if report.Owner != nil {
		if opts.session != "" && opts.session != report.Owner.OperationID {
			return channel.ProjectResult{}, errors.New("live.channel_owner_mismatch: repair must use the original CON")
		}
		if !needsInstall && opts.release == "" {
			return project.Resume(ctx, report.Owner.OperationID, !opts.noCache)
		}
		// Close the original connection without resuming pending business. Older
		// managed files are accepted only by this repair-specific cleanup path.
		if result, err := project.CloseForRepair(ctx, report.Owner.OperationID, !opts.noCache); err != nil {
			return result, err
		}
	}

	if needsInstall || opts.release != "" {
		release := opts.release
		if release == "" {
			release = bundledDoctorRelease()
		}
		if release == "" {
			return channel.ProjectResult{}, errors.New("live.channel_release_required: supply --release with the current distribution")
		}
		var err error
		if deployment.State == "absent" {
			_, err = delivery.InstallAddon(ctx, release, target.Client.Directory, buildinfo.Version)
		} else {
			archive := opts.output
			if archive == "" {
				parent := filepath.Join(project.Root, ".lycheedev", "live", "repairs")
				if !strings.EqualFold(filepath.VolumeName(project.Root), filepath.VolumeName(target.Client.Directory)) {
					// Managed replacement uses directory renames. Keep a cross-volume
					// project's archive on the target volume, outside AddOns discovery.
					parent = filepath.Join(filepath.Dir(filepath.Dir(delivery.AddonDirectory(target.Client.Directory))), ".lycheedev-repairs")
				}
				archive = filepath.Join(parent, time.Now().UTC().Format("20060102T150405.000000000"))
				// Prepare only the parent; managed publication must still refuse an
				// existing archive, and explicit --output paths remain unchanged.
				if err := os.MkdirAll(filepath.Dir(archive), 0700); err != nil {
					return channel.ProjectResult{}, err
				}
			}
			_, err = delivery.UpgradeAddon(ctx, release, target.Client.Directory, archive, buildinfo.Version, false)
		}
		if err != nil {
			return channel.ProjectResult{}, err
		}
		key := opts.request
		if key == "" {
			key = "doctor-" + time.Now().UTC().Format("20060102T150405.000000000")
		}
		return project.Activate(ctx, channel.TargetRequest{Installation: target.Client.Directory, PID: target.Window.ProcessID}, key, !opts.noCache)
	}
	if opts.session != "" {
		return project.Resume(ctx, opts.session, !opts.noCache)
	}
	return project.Connect(ctx, channel.TargetRequest{Installation: target.Client.Directory, PID: target.Window.ProcessID}, !opts.noCache)
}
func bundledDoctorRelease() string {
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	directory := filepath.Dir(exe)
	for depth := 0; depth < 3; depth++ {
		if info, err := os.Stat(filepath.Join(directory, "release.json")); err == nil && info.Mode().IsRegular() {
			return directory
		}
		directory = filepath.Dir(directory)
	}
	return ""
}
func liveDoctorChecks(ctx context.Context, opts Options) []vault.Check {
	if opts.installation == "" && opts.pid == 0 && opts.session == "" {
		return nil
	}
	project, err := channel.OpenProject(opts.project)
	if err != nil {
		return []vault.Check{{ID: "live.project", Status: "error", Detail: err.Error()}}
	}
	bounded, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	report := project.Doctor(bounded, doctorTarget(opts), opts.session)
	checks := make([]vault.Check, 0, len(report.Checks))
	for _, c := range report.Checks {
		status := c.State
		if status == "blocked" {
			status = "error"
		}
		checks = append(checks, vault.Check{ID: c.ID, Status: status, OK: status == "ok", Detail: c.Detail, NextStep: c.NextStep})
	}
	return checks
}
