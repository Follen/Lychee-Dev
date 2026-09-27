package live

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestBootstrapAdmissionSerializesWorkspacesAndMaintenance(t *testing.T) {
	ctx := context.Background()
	target := ClientWindow{Client: selection.ClientInstallation{Directory: bootstrapClientDirectory(t)}, Window: desktop.WindowIdentity{ProcessID: 42, Handle: 99, ProcessStartedAt: 11}}
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	roots := []string{filepath.Join(t.TempDir(), "workspace"), filepath.Join(t.TempDir(), "workspace")}
	for _, root := range roots {
		if _, err := vault.Initialize(ctx, root); err != nil {
			t.Fatal(err)
		}
	}
	type result struct {
		index   int
		attempt BootstrapReceiverAttempt
		err     error
	}
	start := make(chan struct{})
	results := make(chan result, 2)
	for i, root := range roots {
		go func(i int, root string) {
			<-start
			a, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify "+strings.Repeat("a", 32))
			results <- result{i, a, err}
		}(i, root)
	}
	close(start)
	first, second := <-results, <-results
	if first.err != nil {
		first, second = second, first
	}
	if first.err != nil || !errors.Is(second.err, journal.ErrBusy) || second.attempt.ID != "" {
		t.Fatalf("%+v %+v", first, second)
	}
	if gate, err := journal.AcquireInstallationMaintenance(ctx, parent); !errors.Is(err, journal.ErrBusy) {
		if gate != nil {
			gate.Close()
		}
		t.Fatalf("maintenance bypassed first contact: %v", err)
	}
	owner, busy, err := journal.InspectWindowOwner(ctx, parent, windowResource(target))
	if err != nil || !busy || owner.OperationID != first.attempt.ID {
		t.Fatal(owner, busy, err)
	}
	driver, err := journal.LockBootstrapWindow(ctx, parent, owner)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := AbandonBootstrapReceiver(ctx, roots[first.index], first.attempt.ID); !errors.Is(err, journal.ErrBusy) {
		t.Fatalf("abandon raced input: %v", err)
	}
	driver.Close()
	// Another window in the same installation is independent.
	other := target
	other.Window.Handle++
	parallel, err := BeginBootstrapReceiver(ctx, roots[second.index], other, "/dev connect")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := AbandonBootstrapReceiver(ctx, roots[first.index], first.attempt.ID); err != nil {
		t.Fatal(err)
	}
	if gate, err := journal.AcquireInstallationMaintenance(ctx, parent); !errors.Is(err, journal.ErrBusy) {
		if gate != nil {
			gate.Close()
		}
		t.Fatalf("maintenance missed other window: %v", err)
	}
	if _, err := AbandonBootstrapReceiver(ctx, roots[second.index], parallel.ID); err != nil {
		t.Fatal(err)
	}
	gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
	if err != nil {
		t.Fatal(err)
	}
	blocked, cancel := context.WithTimeout(ctx, 30*time.Millisecond)
	_, err = BeginBootstrapReceiver(blocked, roots[0], target, "/dev connect")
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("admission crossed maintenance: %v", err)
	}
	gate.Close()
	if _, err := BeginBootstrapReceiver(ctx, roots[0], target, "/dev connect"); err != nil {
		t.Fatal(err)
	}
}
