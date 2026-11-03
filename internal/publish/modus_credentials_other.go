//go:build !windows

package publish

import "context"

func modusMainClient(context.Context) (*modusClient, error) {
	return nil, capability("publish.modus_session", "main-client session reuse requires Windows")
}

func modusCreatorToken() (string, error) {
	return "", capability("publish.modus_credentials", "Creator session reuse requires Windows CurrentUser DPAPI")
}
