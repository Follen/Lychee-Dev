package delivery

import (
	"bytes"
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

type stopAfterOldMove struct {
	context.Context
	archive, ready string
}

func (c stopAfterOldMove) Err() error {
	if _, err := os.Stat(filepath.Join(c.archive, "previous")); err == nil {
		if err := os.WriteFile(c.ready, []byte("old-moved"), 0600); err != nil {
			return err
		}
		<-c.Context.Done()
	}
	return c.Context.Err()
}

func TestUpgradeWorker(t *testing.T) {
	if os.Getenv("LYCHEEDEV_UPGRADE_WORKER") != "1" {
		t.Skip("subprocess helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	_, err := UpgradeInstallation(stopAfterOldMove{ctx, os.Getenv("LYCHEEDEV_UPGRADE_ARCHIVE"), os.Getenv("LYCHEEDEV_UPGRADE_READY")}, os.Getenv("LYCHEEDEV_UPGRADE_RELEASE"), os.Getenv("LYCHEEDEV_UPGRADE_TARGET"), os.Getenv("LYCHEEDEV_UPGRADE_ARCHIVE"), "skill", "2.0.0-dev")
	if err != nil {
		t.Fatal(err)
	}
}

func TestUpgradeSurvivesKilledProcessAfterOldMove(t *testing.T) {
	source, target, archive, _ := upgradeFixture(t)
	ready := filepath.Join(t.TempDir(), "ready")
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	command := exec.CommandContext(ctx, executable, "-test.run=^TestUpgradeWorker$", "-test.count=1")
	command.Env = append(os.Environ(), "LYCHEEDEV_UPGRADE_WORKER=1", "LYCHEEDEV_UPGRADE_RELEASE="+source, "LYCHEEDEV_UPGRADE_TARGET="+target, "LYCHEEDEV_UPGRADE_ARCHIVE="+archive, "LYCHEEDEV_UPGRADE_READY="+ready)
	var output bytes.Buffer
	command.Stdout = &output
	command.Stderr = &output
	if err := command.Start(); err != nil {
		cancel()
		t.Fatal(err)
	}
	done := make(chan struct{})
	var waitErr error
	go func() { waitErr = command.Wait(); close(done) }()
	defer func() { cancel(); <-done }()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
waiting:
	for {
		select {
		case <-done:
			t.Fatalf("worker exited before checkpoint: %v %s", waitErr, output.String())
		case <-ticker.C:
			if _, err := os.Stat(ready); err == nil {
				break waiting
			}
		}
	}
	if err := command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	<-done
	if waitErr == nil {
		t.Fatal("worker was not terminated")
	}
	if _, err := os.Stat(target); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("not at move checkpoint: %v", err)
	}
	recoveryCtx, recoveryCancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer recoveryCancel()
	if _, err := ResumeUpgrade(recoveryCtx, target, archive, "skill"); err != nil {
		t.Fatal(err)
	}
	assessment, err := InspectInstallation(context.Background(), target, "skill")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("%+v %v", assessment, err)
	}
}
