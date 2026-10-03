//go:build !windows || !amd64

package command

import (
 "context"
 "errors"
 "strings"
)

func runChannelCommand(_ context.Context, route, _ string, _ Options, _ *Envelope) (bool, int, error) {
	if strings.HasPrefix(route,"live ") && route!="live instances" && !strings.HasPrefix(route,"live probe ") {
		return true,3,errors.New("duplex transport requires Windows amd64")
	}
	return false, 0, nil
}
