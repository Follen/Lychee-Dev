package journal

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestWindowRunCheckRejectsChangedMarker(t *testing.T) {
	ctx := context.Background()
	book, store := windowBook(t)
	parent := t.TempDir()
	record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, windowIntent("window/1/2/3"))
	if err != nil {
		t.Fatal(err)
	}
	run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer run.Close()
	marker := filepath.Join(parent, ".lycheedev-window-owners", fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	owner := run.owner
	owner.OperationID = "OP-00000000000000000000000000000000"
	raw, _ := json.Marshal(owner)
	if err := os.WriteFile(marker, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := run.Check(ctx); !errors.Is(err, ErrBusy) {
		t.Fatal("changed owner accepted", err)
	}
}

func TestWindowRunOwnershipAndTerminalRelease(t *testing.T) {
	ctx := context.Background()
	book, store := windowBook(t)
	parent := t.TempDir()
	record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, windowIntent("window/1/2/3"))
	if err != nil {
		t.Fatal(err)
	}
	run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer run.Close()
	if _, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrBusy) {
		t.Fatalf("duplicate driver: %v", err)
	}
	if current, err := run.Check(ctx); err != nil || current.Stage != "prepared" {
		t.Fatalf("check: %+v %v", current, err)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := run.Check(cancelled); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	// Job state fixture, not a claim of actual game cleanup.
	for _, stage := range []string{"load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted", "verified", "ack_requested", "acknowledged", "cleaned"} {
		status := "running"
		if stage == "cleaned" {
			status = "completed"
		}
		if err := book.AdvanceStage(ctx, StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: json.RawMessage(`null`)}); err != nil {
			t.Fatal(err)
		}
		record, err = book.InspectWork(ctx, record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
	}
	if _, err := run.Check(ctx); !errors.Is(err, ErrTransition) {
		t.Fatal("terminal run still usable", err)
	}
	if err := book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrBusy) {
		t.Fatal("released marker while driver still held execution", err)
	}
	if err := run.Close(); err != nil {
		t.Fatal(err)
	}
	if err := run.Close(); err != nil {
		t.Fatal("close not idempotent", err)
	}
	if _, err := run.Check(ctx); err == nil {
		t.Fatal("closed driver usable")
	}
	if err := book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); err != nil {
		t.Fatal(err)
	}
}

func TestWindowRunProcessHelper(t *testing.T) {
	root := os.Getenv("LYCHEEDEV_RUN_TEST_HOME")
	if root == "" {
		t.Skip("subprocess helper")
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	run, err := OpenBook(metadata).AcquireWindowWork(context.Background(), os.Getenv("LYCHEEDEV_RUN_TEST_PARENT"), store.Identity().WorkspaceID, os.Getenv("LYCHEEDEV_RUN_TEST_OPERATION"))
	if err != nil {
		t.Fatal(err)
	}
	defer run.Close()
	fmt.Println("WINDOW-RUN held")
	_, _ = io.Copy(io.Discard, os.Stdin)
}

func TestWindowRunProcessExitAllowsOnlyRecordedRecovery(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	book, store := windowBook(t)
	record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, windowIntent("window/1/2/3"))
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=^TestWindowRunProcessHelper$")
	cmd.Env = append(os.Environ(), "LYCHEEDEV_RUN_TEST_HOME="+store.Root(), "LYCHEEDEV_RUN_TEST_PARENT="+parent, "LYCHEEDEV_RUN_TEST_OPERATION="+record.OperationID)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	defer stdin.Close()
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = cmd.Process.Kill(); _ = cmd.Wait() }()
	held := make(chan bool, 1)
	go func() {
		scanner := bufio.NewScanner(stdout)
		held <- scanner.Scan() && scanner.Text() == "WINDOW-RUN held"
	}()
	select {
	case ok := <-held:
		if !ok {
			t.Fatal("child did not acquire execution lease")
		}
	case <-time.After(10 * time.Second):
		t.Fatal("child handshake timed out")
	}
	if _, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrBusy) {
		t.Fatal("second process bypassed active driver", err)
	}
	if _, err := book.InspectWork(ctx, record.OperationID); err != nil {
		t.Fatal("driver blocked read", err)
	}
	if _, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, windowIntent("window/1/2/4")); err != nil {
		t.Fatal("driver blocked independent work", err)
	}
	if err := cmd.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	_ = cmd.Wait()
	run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID)
	if err != nil {
		t.Fatal("OS lease survived process exit", err)
	}
	defer run.Close()
	if current, err := run.Check(ctx); err != nil || current.Stage != "prepared" {
		t.Fatal("recovery advanced or replayed work", err)
	}
	other, otherStore := windowBook(t)
	_, err = other.BeginWindowWork(ctx, parent, otherStore.Identity().WorkspaceID, windowIntent("window/1/2/3"))
	var occupied *WindowOccupied
	if !errors.As(err, &occupied) || !occupied.Foreign {
		t.Fatal("process exit erased durable owner", err)
	}
}
