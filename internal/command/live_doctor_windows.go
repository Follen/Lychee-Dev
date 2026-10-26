//go:build windows && amd64

package command

import (
	"context"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/vault"
)

func doctorLive(ctx context.Context, opts Options, response *Envelope) ([]vault.Check, any, error) {
	if !opts.fix && opts.installation == "" && opts.pid == 0 && opts.session == "" {
		return nil, nil, nil
	}
	project, err := channel.OpenProject(opts.project)
	if err != nil {
		return []vault.Check{{ID: "live.project", Status: "error", Detail: err.Error()}}, nil, nil
	}
	wait := opts.waitSeconds
	if wait == 0 {
		wait = 30
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(wait)*time.Second)
	defer cancel()
	target := channel.TargetRequest{Installation: opts.installation, PID: opts.pid}
	var report channel.DoctorResult
	if opts.fix {
		report, err = project.RepairDoctor(ctx, target, opts.session, channel.DoctorRepairOptions{
			Release: opts.release, Archive: opts.output, Request: opts.request, HardReset: opts.hardReset,
		})
	} else {
		report = project.Doctor(ctx, target, opts.session)
	}
	response.Context["project"] = project.Root
	response.Context["protocol"] = channel.ProtocolName
	if report.Connection != nil {
		response.OperationID = report.Connection.Session
	}
	checks := make([]vault.Check, 0, len(report.Checks))
	for _, c := range report.Checks {
		status := c.State
		if status == "blocked" {
			status = "error"
		}
		checks = append(checks, vault.Check{ID: c.ID, Status: status, OK: status == "ok", Detail: c.Detail, NextStep: c.NextStep})
	}
	if errors.Is(err, channel.ErrDoctorUnhealthy) || (err == nil && !report.Healthy) {
		err = errDoctorUnhealthy
	} else if err != nil {
		err = classifyChannelResultError(err)
	}
	return checks, report, err
}
