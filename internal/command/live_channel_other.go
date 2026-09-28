//go:build !windows || !amd64

package command

import "context"

func runChannelCommand(context.Context, string, string, Options, *Envelope) (bool, int, error) {
	return false, 0, nil
}
