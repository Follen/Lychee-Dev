package vault

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestAcquireLeaseIsExclusiveForSameScopeAndResource(t *testing.T) {
	scope := t.TempDir()
	holder, err := AcquireLease(context.Background(), scope, "same-resource")
	if err != nil {
		t.Fatalf("first AcquireLease() error = %v", err)
	}
	defer holder.Close()

	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	defer cancel()
	contender, err := AcquireLease(ctx, scope, "same-resource")
	if contender != nil {
		contender.Close()
		t.Fatal("second AcquireLease() unexpectedly acquired the same lease")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("second AcquireLease() error = %v, want context deadline exceeded", err)
	}
}

func TestTryAcquireLeaseDoesNotQueue(t *testing.T) {
	scope := t.TempDir()
	holder, err := TryAcquireLease(context.Background(), scope, "try")
	if err != nil {
		t.Fatal(err)
	}
	defer holder.Close()
	done := make(chan error, 1)
	go func() {
		lease, err := TryAcquireLease(context.Background(), scope, "try")
		if lease != nil {
			lease.Close()
		}
		done <- err
	}()
	select {
	case err := <-done:
		if !errors.Is(err, ErrLeaseBusy) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("try acquisition queued")
	}
}

func TestAcquireLeaseAllowsIndependentResourcesAndScopes(t *testing.T) {
	scopeOne := t.TempDir()
	scopeTwo := t.TempDir()

	resourceOne, err := AcquireLease(context.Background(), scopeOne, "resource-one")
	if err != nil {
		t.Fatalf("resource-one in scope one: %v", err)
	}
	defer resourceOne.Close()

	resourceTwo, err := AcquireLease(context.Background(), scopeOne, "resource-two")
	if err != nil {
		t.Fatalf("resource-two in scope one: %v", err)
	}
	defer resourceTwo.Close()

	otherScope, err := AcquireLease(context.Background(), scopeTwo, "resource-one")
	if err != nil {
		t.Fatalf("resource-one in scope two: %v", err)
	}
	defer otherScope.Close()
}

func TestAcquireLeaseHonorsCancellationWhileWaiting(t *testing.T) {
	scope := t.TempDir()
	holder, err := AcquireLease(context.Background(), scope, "cancelled-resource")
	if err != nil {
		t.Fatalf("holder AcquireLease() error = %v", err)
	}
	defer holder.Close()

	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		contender, acquireErr := AcquireLease(ctx, scope, "cancelled-resource")
		if contender != nil {
			_ = contender.Close()
		}
		result <- acquireErr
	}()

	select {
	case err := <-result:
		t.Fatalf("contender returned before cancellation: %v", err)
	case <-time.After(100 * time.Millisecond):
	}
	cancel()

	select {
	case err := <-result:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("contender error = %v, want context canceled", err)
		}
	case <-time.After(time.Second):
		t.Fatal("contender did not stop after context cancellation")
	}
}

func TestAcquireLeaseAfterKilledHolderProcess(t *testing.T) {
	scope := t.TempDir()
	cmd := exec.Command(os.Args[0], "-test.run=^TestLeaseHelperProcess$", "-test.v=false")
	cmd.Env = append(os.Environ(),
		"LYCHEE_DEV_LEASE_HELPER=1",
		"LYCHEE_DEV_LEASE_SCOPE="+scope,
		"LYCHEE_DEV_LEASE_RESOURCE=killed-holder",
	)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatalf("StdoutPipe() error = %v", err)
	}
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		t.Fatalf("start lease helper: %v", err)
	}
	stopped := false
	defer func() {
		if !stopped {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
		}
	}()

	ready := make(chan error, 1)
	go func() {
		line, readErr := bufio.NewReader(stdout).ReadString('\n')
		if readErr != nil {
			ready <- readErr
			return
		}
		if strings.TrimSpace(line) != "ready" {
			ready <- fmt.Errorf("unexpected helper output %q", strings.TrimSpace(line))
			return
		}
		ready <- nil
	}()
	select {
	case err := <-ready:
		if err != nil {
			t.Fatalf("lease helper did not become ready: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for lease helper")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	blocked, err := AcquireLease(ctx, scope, "killed-holder")
	cancel()
	if blocked != nil {
		blocked.Close()
		t.Fatal("parent acquired lease while helper process held it")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("parent waiting AcquireLease() error = %v, want context deadline exceeded", err)
	}

	if err := cmd.Process.Kill(); err != nil {
		t.Fatalf("kill lease helper: %v", err)
	}
	_ = cmd.Wait()
	stopped = true

	ctx, cancel = context.WithTimeout(context.Background(), 2*time.Second)
	freed, err := AcquireLease(ctx, scope, "killed-holder")
	cancel()
	if err != nil {
		t.Fatalf("AcquireLease() after killing holder: %v", err)
	}
	if freed == nil {
		t.Fatal("AcquireLease() after killing holder returned a nil lease")
	}
	defer freed.Close()
}

func TestLeaseHelperProcess(t *testing.T) {
	if os.Getenv("LYCHEE_DEV_LEASE_HELPER") != "1" {
		return
	}

	lease, err := AcquireLease(context.Background(), os.Getenv("LYCHEE_DEV_LEASE_SCOPE"), os.Getenv("LYCHEE_DEV_LEASE_RESOURCE"))
	if err != nil {
		fmt.Fprintf(os.Stdout, "helper error: %v\n", err)
		return
	}
	defer lease.Close()
	if _, err := fmt.Fprintln(os.Stdout, "ready"); err != nil {
		t.Fatalf("signal helper readiness: %v", err)
	}
	time.Sleep(10 * time.Minute)
}
