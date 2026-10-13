//go:build !windows || !amd64

package command

import (
	"context"
	"github.com/follenfang/lycheedev/internal/vault"
)

func liveDoctorChecks(_ context.Context, opts Options) []vault.Check {
	if opts.installation == "" && opts.pid == 0 && opts.session == "" {
		return nil
	}
	return []vault.Check{{ID: "live.platform", Status: "error", Detail: "Live requires Windows amd64"}}
}
