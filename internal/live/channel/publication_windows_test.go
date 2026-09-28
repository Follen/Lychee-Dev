//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func publicationFixture(t *testing.T) (*Native, bridge.SlotEnvelope) {
	t.Helper()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(context.Background(), testkit.Release(t, "slot-runtime"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	n := &Native{Parent: filepath.Join(client, "Interface", "AddOns"), Version: testkit.Version, Consumer: "1/1", Guard: func(context.Context) error { return nil }}
	t.Cleanup(func() {
		if n.Publication != nil {
			_ = n.Publication.Close()
		}
	})
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
	return n, e
}

func TestSharedPublicationWaitsAndRechecksAdmission(t *testing.T) {
	n, e := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	held, err := vault.AcquireLease(ctx, filepath.Join(n.Parent, ".lycheedev-slot-locks"), "publication")
	if err != nil {
		t.Fatal(err)
	}
	defer held.Close()
	entered := make(chan struct{})
	done := make(chan error, 1)
	guards := 0
	rejected := errors.New("owner changed while queued")
	n.Guard = func(context.Context) error {
		guards++
		if guards == 1 {
			close(entered)
			return nil
		}
		return rejected
	}
	go func() { done <- n.Publish(ctx, e) }()
	<-entered
	select {
	case err := <-done:
		t.Fatalf("did not queue: %v", err)
	case <-time.After(75 * time.Millisecond):
	}
	if err = held.Close(); err != nil {
		t.Fatal(err)
	}
	if err = <-done; !errors.Is(err, rejected) {
		t.Fatalf("post-wait guard: %v", err)
	}
	pool, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
	if err != nil || pool.Files[0].Nonce != "" {
		t.Fatal("published after admission changed", err)
	}
}

func TestPublicationDeadlineDoesNotPublishOrAcquireInput(t *testing.T) {
	n, e := publicationFixture(t)
	held, err := vault.AcquireLease(context.Background(), filepath.Join(n.Parent, ".lycheedev-slot-locks"), "publication")
	if err != nil {
		t.Fatal(err)
	}
	defer held.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
	defer cancel()
	if err = n.Publish(ctx, e); !errors.Is(err, ErrPublicationPending) || n.Publication != nil {
		t.Fatalf("%v", err)
	}
	pool, err := delivery.InspectSlots(context.Background(), n.Parent, n.Version)
	if err != nil || pool.Files[0].Nonce != "" {
		t.Fatal("changed payload while waiting", err)
	}
}

func TestUnresolvedPublicationReleasesLockForOriginalOwner(t *testing.T) {
	n, a := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := n.Publish(ctx, a); err != nil {
		t.Fatal(err)
	}
	// Original CLI exits after publishing but before reading its receipt.
	if n.Publication != nil {
		t.Fatal("publication retained global lease")
	}
	b := a
	b.Nonce = strings.Repeat("5", 32)
	b.Runtime = strings.Repeat("6", 32)
	second := &Native{Parent: n.Parent, Version: n.Version, Consumer: "2/2", Guard: n.Guard}
	defer func() {
		if second.Publication != nil {
			_ = second.Publication.Close()
		}
	}()
	path := filepath.Join(delivery.SlotDirectory(n.Parent, 1), "Payload.lua")
	before, _ := os.ReadFile(path)
	if err := second.Publish(ctx, b); !errors.Is(err, ErrPublicationPending) || second.Publication != nil {
		t.Fatalf("reservation did not yield: %v", err)
	}
	after, _ := os.ReadFile(path)
	if string(before) != string(after) {
		t.Fatal("overwrote unresolved payload")
	}
	// Original receipt can now be reconciled. Model a crash after pool save but
	// before the local transaction advance, then reuse this physical slot in B.
	if err := n.Consumed(ctx, a); err != nil {
		t.Fatal(err)
	}
	if err := second.Publish(ctx, b); err != nil {
		t.Fatal(err)
	}
	if second.Publication != nil {
		t.Fatal("publication retained global lease")
	}
	if err := n.Consumed(ctx, a); err != nil {
		t.Fatal("durably verified receipt could not resume", err)
	}
	pool, err := delivery.InspectSlots(ctx, n.Parent, n.Version)
	if err != nil || pool.Files[0].Nonce != b.Nonce || pool.Files[0].Consumed {
		t.Fatal("retired another consumer", err)
	}
}

func TestResumedPublicationMustMatchBytesAndRemainUnconsumed(t *testing.T) {
	n, e := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := n.Publish(ctx, e); err != nil {
		t.Fatal(err)
	}
	if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, e); err != nil {
		t.Fatal(err)
	}
	changed := e
	changed.Ticket = strings.Repeat("9", 32)
	if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, changed); err == nil {
		t.Fatal("same nonce accepted different payload")
	}
	if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, "other", e); err == nil {
		t.Fatal("accepted foreign consumer")
	}
	if err := n.Consumed(ctx, e); err != nil {
		t.Fatal(err)
	}
	if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, e); err == nil {
		t.Fatal("consumed input was resendable")
	}
}

func TestWaitingRuntimeReservesOnlyItsOwnSlot(t *testing.T) {
	n, a := publicationFixture(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := n.Publish(ctx, a); err != nil {
		t.Fatal(err)
	}
	if n.Publication != nil {
		t.Fatal("game readiness holds installation lock")
	}
	b := a
	b.Index = 2
	b.Nonce = strings.Repeat("5", 32)
	b.Runtime = strings.Repeat("6", 32)
	other := &Native{Parent: n.Parent, Version: n.Version, Consumer: "2/2", Guard: n.Guard}
	if err := other.Publish(ctx, b); err != nil {
		t.Fatal("unrelated slot blocked", err)
	}
	if err := delivery.VerifySlotPublication(ctx, n.Parent, n.Version, n.Consumer, a); err != nil {
		t.Fatal("original reservation lost", err)
	}
}
