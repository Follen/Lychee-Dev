package delivery

import (
	"bytes"
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestInstallationWorker(t *testing.T) {
	if os.Getenv("LYCHEEDEV_INSTALL_WORKER") != "1" {
		t.Skip("subprocess helper")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	for {
		if _, err := os.Stat(os.Getenv("LYCHEEDEV_INSTALL_BARRIER")); err == nil {
			break
		}
		select {
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		case <-time.After(10 * time.Millisecond):
		}
	}
	_, err := InstallFresh(ctx, os.Getenv("LYCHEEDEV_INSTALL_RELEASE"), os.Getenv("LYCHEEDEV_INSTALL_TARGET"), "skill", "2.0.0-dev")
	if err != nil {
		t.Fatal(err)
	}
}

func TestIndependentProcessesInstallSameTarget(t *testing.T) {
	release, _ := releaseFixture(t)
	parent := t.TempDir()
	target := filepath.Join(parent, "lycheedev")
	barrier := filepath.Join(parent, "start")
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	type worker struct {
		command *exec.Cmd
		output  bytes.Buffer
	}
	workers := make([]*worker, 4)
	for i := range workers {
		w := &worker{command: exec.CommandContext(ctx, executable, "-test.run=^TestInstallationWorker$", "-test.count=1")}
		w.command.Env = append(os.Environ(), "LYCHEEDEV_INSTALL_WORKER=1", "LYCHEEDEV_INSTALL_RELEASE="+release, "LYCHEEDEV_INSTALL_TARGET="+target, "LYCHEEDEV_INSTALL_BARRIER="+barrier)
		w.command.Stdout = &w.output
		w.command.Stderr = &w.output
		if err := w.command.Start(); err != nil {
			t.Fatal(err)
		}
		workers[i] = w
	}
	if err := os.WriteFile(barrier, []byte("start"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, w := range workers {
		if err := w.command.Wait(); err != nil {
			t.Errorf("worker: %v: %s", err, w.output.String())
		}
	}
	assessment, err := InspectInstallation(context.Background(), target, "skill")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("%+v %v", assessment, err)
	}
}
