//go:build !windows

package luals

import "os/exec"

func hideProcess(command *exec.Cmd) {}
func guardProcessPolicy(command *exec.Cmd, broker bool) (func(), error) {
	return func() { _ = command.Process.Kill() }, nil
}

func prepareGuarded(command *exec.Cmd) {}
