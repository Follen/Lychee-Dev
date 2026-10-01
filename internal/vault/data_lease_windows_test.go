//go:build windows

package vault

import (
	"bufio"
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// AfterFunc observes Done only after LockFileEx has returned IO_PENDING. This
// lets the test synchronize with the actual queued kernel operation.
type observedDataContext struct {
	context.Context
	once   sync.Once
	queued chan struct{}
}

func (c *observedDataContext) Done() <-chan struct{} {
	c.once.Do(func() { close(c.queued) })
	return c.Context.Done()
}

func TestDataLeasePendingReleaseAndCancellation(t *testing.T) {
	for _, canceled := range []bool{false, true} {
		scope := t.TempDir()
		holder, err := acquireDataLease(context.Background(), scope, "pending")
		if err != nil {
			t.Fatal(err)
		}
		base, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		ctx := &observedDataContext{Context: base, queued: make(chan struct{})}
		type result struct {
			lease *Lease
			err   error
		}
		done := make(chan result, 1)
		go func() { lease, err := acquireDataLease(ctx, scope, "pending"); done <- result{lease, err} }()
		select {
		case <-ctx.queued:
		case <-base.Done():
			holder.Close()
			cancel()
			t.Fatal("lock never queued")
		}
		if canceled {
			cancel()
		} else if err = holder.Close(); err != nil {
			cancel()
			t.Fatal(err)
		}
		select {
		case got := <-done:
			if canceled {
				if got.lease != nil || !errors.Is(got.err, context.Canceled) {
					t.Fatalf("canceled lease=%v err=%v", got.lease, got.err)
				}
				if err = holder.Close(); err != nil {
					t.Fatal(err)
				}
			} else {
				if got.err != nil {
					t.Fatal(got.err)
				}
				if err = got.lease.Close(); err != nil {
					t.Fatal(err)
				}
			}
		case <-time.After(time.Second):
			cancel()
			holder.Close()
			t.Fatal("pending operation did not finish")
		}
		cancel()
		next, err := TryAcquireLease(context.Background(), scope, "pending")
		if err != nil {
			t.Fatalf("operation retained a lock: %v", err)
		}
		next.Close()
	}
}

func TestDataLeaseCancellationReleaseRace(t *testing.T) {
	scope := t.TempDir()
	for i := 0; i < 64; i++ {
		holder, err := acquireDataLease(context.Background(), scope, "race")
		if err != nil {
			t.Fatal(err)
		}
		base, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		ctx := &observedDataContext{Context: base, queued: make(chan struct{})}
		done := make(chan error, 1)
		go func() {
			lease, err := acquireDataLease(ctx, scope, "race")
			if lease != nil {
				err = errors.Join(err, lease.Close())
			}
			done <- err
		}()
		select {
		case <-ctx.queued:
		case <-base.Done():
			holder.Close()
			cancel()
			t.Fatal("lock never queued")
		}
		closed := make(chan error, 1)
		go func() { closed <- holder.Close() }()
		cancel()
		if err = <-closed; err != nil {
			t.Fatal(err)
		}
		select {
		case err = <-done:
			if err != nil && !errors.Is(err, context.Canceled) {
				t.Fatal(err)
			}
		case <-time.After(time.Second):
			t.Fatal("cancellation/release race did not finish")
		}
		next, err := TryAcquireLease(context.Background(), scope, "race")
		if err != nil {
			t.Fatalf("iteration %d retained a lock: %v", i, err)
		}
		next.Close()
	}
}

func TestDataLeaseQueuedOwnerDeath(t *testing.T) {
	store, err := Initialize(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	command := exec.Command(os.Args[0], "-test.run=^TestDataResourceHelperProcess$")
	command.Env = append(os.Environ(), "LYCHEE_DEV_DATA_RESOURCE_HELPER="+store.Root())
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	command.Stderr = os.Stderr
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = command.Process.Kill(); _ = command.Wait() }()
	ready := make(chan string, 1)
	go func() { line, _ := bufio.NewReader(stdout).ReadString('\n'); ready <- strings.TrimSpace(line) }()
	select {
	case line := <-ready:
		if line != "ready" {
			t.Fatal(line)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("holder never ready")
	}
	base, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	ctx := &observedDataContext{Context: base, queued: make(chan struct{})}
	done := make(chan error, 1)
	go func() {
		lease, err := acquireDataLease(ctx, filepath.Join(store.Root(), "locks", "data-resources-v1"), "admission")
		if lease != nil {
			err = errors.Join(err, lease.Close())
		}
		done <- err
	}()
	select {
	case <-ctx.queued:
	case <-base.Done():
		t.Fatal("kernel waiter never queued")
	}
	if err = command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	select {
	case err = <-done:
		if err != nil {
			t.Fatalf("holder death did not release queued lease: %v", err)
		}
	case <-base.Done():
		t.Fatal("holder death did not wake queued operation")
	}
}
