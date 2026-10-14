//go:build windows

package testkit

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSourceBrokerGateSerializesDifferentHomes(t *testing.T) {
	first, second := t.TempDir(), t.TempDir()
	child := func(t *testing.T, mode string) {
		t.Helper()
		command := exec.CommandContext(t.Context(), os.Args[0], "-test.run=^TestSourceBrokerGateHelperProcess$")
		command.Env = append(os.Environ(), "LYCHEEDEV_TEST_BROKER_GATE="+mode, "LYCHEEDEV_TEST_BROKER_GATE_HOME="+second)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("%s contender: %v: %s", mode, err, output)
		}
	}
	// The gate has to coordinate distinct homes in different package processes,
	// not merely two callers in this process or one workspace's owner lock.
	t.Run("owner", func(t *testing.T) {
		SerializeSourceBroker(t, first)
		_, _, scope, err := luals.BrokerScope(first)
		if err != nil {
			t.Fatal(err)
		}
		aggregate, err := vault.TryAcquireLease(t.Context(), scope, "source-broker-aggregate.v1")
		if err != nil {
			t.Fatalf("test gate claimed production ownership: %v", err)
		}
		t.Cleanup(func() { _ = aggregate.Close() })
		child(t, "blocked")
	})
	// t.Run joins all registered cleanup, including gate release.
	child(t, "released")
}

func TestSourceBrokerGateHelperProcess(t *testing.T) {
	mode := os.Getenv("LYCHEEDEV_TEST_BROKER_GATE")
	if mode == "" {
		t.Skip("subprocess helper; exercised by TestSourceBrokerGateSerializesDifferentHomes")
	}
	ctx := t.Context()
	if mode == "blocked" {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, 100*time.Millisecond)
		defer cancel()
	}
	home := os.Getenv("LYCHEEDEV_TEST_BROKER_GATE_HOME")
	gate, err := sourceBrokerTestGate(ctx, home)
	if gate != nil {
		defer gate.Close()
	}
	if mode == "blocked" {
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("different home bypassed occupied logon gate: %v", err)
		}
	} else if mode != "released" || err != nil {
		t.Fatalf("gate after owner cleanup: mode=%q err=%v", mode, err)
	} else {
		_, _, scope, err := luals.BrokerScope(home)
		if err != nil {
			t.Fatal(err)
		}
		aggregate, err := vault.TryAcquireLease(ctx, scope, "source-broker-aggregate.v1")
		if err != nil {
			t.Fatalf("test gate released before broker ownership cleanup: %v", err)
		}
		_ = aggregate.Close()
	}
}
