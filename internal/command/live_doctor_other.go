//go:build !windows || !amd64

package command

import (
	"context"
	"github.com/follenfang/lycheedev/internal/vault"
)

func liveDoctorChecks(_ context.Context, opts Options) []vault.Check {
	if opts.session == "" && opts.installation == "" && opts.pid == 0 {
		return nil
	}
	return []vault.Check{{ID: "live.duplex", OK: false, Status: "error", Detail: "duplex transport requires Windows amd64"}}
}
