//go:build !windows || !amd64

package publish

import (
	"context"
	"net"
	"os/exec"
)

func ddUnsupported() error {
	return capability("publish.dd_platform", "DD native sessions require Windows amd64")
}
func ddHandleChannelHelper(context.Context, []string) (bool, int)      { return false, 0 }
func ddPipeName(id string) string                                      { return "dd-" + id }
func ddPrivateDirectory(string) error                                  { return ddUnsupported() }
func ddConfigureWorker(*exec.Cmd)                                      {}
func ddProcessCreated(int) (uint64, error)                             { return 0, ddUnsupported() }
func ddWorkerAlive(ddSessionState) (bool, error)                       { return false, ddUnsupported() }
func ddListenSession(ddSessionState) (net.Listener, error)             { return nil, ddUnsupported() }
func ddDialSession(context.Context, ddSessionState) (net.Conn, error)  { return nil, ddUnsupported() }
func ddInstallation(context.Context, string) (string, error)           { return "", ddUnsupported() }
func ddOpenTaskSession(context.Context, string, string) (ddAPI, error) { return nil, ddUnsupported() }
