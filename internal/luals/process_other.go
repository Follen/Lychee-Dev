//go:build !windows

package luals

import "os/exec"

func hideProcess(command *exec.Cmd) {}
func guardProcess(command *exec.Cmd) (func(), error) {
	return func() { _ = command.Process.Kill() }, nil
}
