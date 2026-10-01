package luals

import (
	"context"
	"os/exec"
)

func startGuarded(command *exec.Cmd) (func(), error) { return startGuardedPolicy(command, false) }
func startGuardedPolicy(command *exec.Cmd, broker bool) (func(), error) {
	prepareGuarded(command)
	if err := command.Start(); err != nil {
		return nil, err
	}
	closeJob, err := guardProcessPolicy(command, broker)
	if err != nil {
		_ = command.Process.Kill()
		_ = command.Wait()
		return nil, err
	}
	return closeJob, nil
}

func runGuarded(ctx context.Context, command *exec.Cmd) error {
	closeJob, err := startGuarded(command)
	if err != nil {
		return err
	}
	defer closeJob()
	done := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			closeJob()
		case <-done:
		}
	}()
	err = command.Wait()
	close(done)
	return err
}
