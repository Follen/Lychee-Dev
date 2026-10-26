//go:build !windows || !amd64

package command

import (
	"context"
	"github.com/follenfang/lycheedev/internal/vault"
)

func doctorLive(_ context.Context, opts Options, _ *Envelope) ([]vault.Check, any, error) {
	if !opts.fix && opts.installation == "" && opts.pid == 0 && opts.session == "" {
		return nil, nil, nil
	}
	return []vault.Check{{ID: "live.platform", Status: "error", Detail: "Live requires Windows amd64"}}, nil, nil
}
