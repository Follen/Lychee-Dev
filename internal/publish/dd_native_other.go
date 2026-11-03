//go:build !windows || !amd64

package publish

import "context"

func openDDSigner(context.Context, string) (ddSigner, error) {
	return nil, capability("publish.dd_platform", "DD native SDK requires Windows amd64")
}
