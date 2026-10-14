//go:build windows

package luals

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"
)

func TestBrokerExitWaitKeepsLifetimeAndCancellation(t *testing.T) {
	pid := uint32(os.Getpid())
	started, _, err := BrokerProcessIdentity(pid)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	if err = WaitBrokerExit(ctx, pid, started); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("live process wait ignored deadline: %v", err)
	}
	// A reused PID must not make the caller wait for the unrelated lifetime.
	if err = WaitBrokerExit(context.Background(), pid, started-1); err != nil {
		t.Fatalf("original lifetime already ended: %v", err)
	}
	if got, _, err := BrokerProcessIdentity(pid); err != nil || got != started {
		t.Fatalf("waiting changed live process identity: %d %v", got, err)
	}
}
