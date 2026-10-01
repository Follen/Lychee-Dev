//go:build windows

package luals

import (
	"os/exec"
	"syscall"
)

func newBrokerCommand(exe string, args []string) *exec.Cmd {
	command := exec.Command(exe, args...)
	command.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return command
}
