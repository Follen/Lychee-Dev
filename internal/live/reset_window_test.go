package live

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func resetReceipt(nonce string, mutate func(*bridge.Signal)) bridge.Signal {
	signal := identityReceipt(nonce, nil)
	signal.Kind = "reset"
	signal.SessionNonce = ""
	signal.Sequence = 0
	signal.InputReady = false
	signal.ActorState = ""
	if mutate != nil {
		mutate(&signal)
	}
	return signal
}

func TestResetUnblocksBusyQueueAndConnects(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	// The runtime is stuck: identity triggers stay unanswered (identity_busy
	// prints in-game but no receipt is displayed). Only the fixed reset
	// trigger gets a nonce-correlated receipt.
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if strings.HasPrefix(command, "/dev bridge reset ") {
			return []*desktop.CapturedFrame{fake.frame(1, resetReceipt(nonceFromCommand(command), nil))}
		}
		if command == "/dev connect" {
			return []*desktop.CapturedFrame{
				fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence = false, 1 })),
				fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence = true, 2 })),
			}
		}
		return identityResponder(fake, 1, nil)(command)
	}
	outcome, err := resetWindow(context.Background(), root, ResetRequest{Snapshot: pin}, fake.io())
	if err != nil || !outcome.Reset || outcome.Connection.ID == "" {
		t.Fatalf("reset: %+v %v commands=%v", outcome, err, fake.commands())
	}
	if outcome.Connection.Character != "Paladin" || outcome.Connection.Realm != "Realm" {
		t.Fatalf("connection identity: %+v", outcome.Connection)
	}
	sent := fake.sentKind("/dev bridge reset")
	if len(sent) != 1 || len(fake.sentKind("/dev connect")) != 1 {
		t.Fatalf("input sequence: %v", fake.commands())
	}
	if nonce := nonceFromCommand(strings.TrimPrefix(sent[0], "1:")); len(nonce) != 32 || strings.Trim(nonce, "0123456789abcdef") != "" {
		t.Fatalf("reset nonce: %q", nonce)
	}
	saved, err := ReadWindowSession(context.Background(), root, outcome.Connection.ID)
	if err != nil || saved.Ready.Character != "Paladin" {
		t.Fatalf("persisted session: %+v %v", saved, err)
	}
}

func TestResetRefusesOwnedWindow(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	fake.owners[windowResource(ClientWindow{Window: testWindow(1, client)})] = journal.WindowOwner{
		Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("a", 32),
		Resource: windowResource(ClientWindow{Window: testWindow(1, client)}), OperationID: "OP-" + strings.Repeat("b", 32),
	}
	_, err := resetWindow(context.Background(), root, ResetRequest{Snapshot: pin}, fake.io())
	if err == nil || len(fake.commands()) != 0 {
		t.Fatalf("owned window must not receive reset input: %v %v", err, fake.commands())
	}
}

func TestResetStaysPendingWithoutReceipt(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	// No responder: the reset trigger lands but no receipt ever displays.
	fake.respond[1] = func(string) []*desktop.CapturedFrame { return nil }
	_, err := resetWindow(context.Background(), root, ResetRequest{Snapshot: pin}, fake.io())
	if err == nil {
		t.Fatal("absent receipt must not claim a reset")
	}
	sent := fake.sentKind("/dev bridge reset")
	if len(sent) != 1 {
		t.Fatalf("reset triggers: %v", fake.commands())
	}
}

func TestResetRejectsForeignReceiptNonce(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if strings.HasPrefix(command, "/dev bridge reset ") {
			// Echo a different nonce: this receipt proves nothing about the
			// trigger and must never satisfy the wait.
			return []*desktop.CapturedFrame{fake.frame(1, resetReceipt(strings.Repeat("f", 32), nil))}
		}
		return nil
	}
	_, err := resetWindow(context.Background(), root, ResetRequest{Snapshot: pin}, fake.io())
	var missing *CandidateSelectionError
	if err == nil || errors.As(err, &missing) && missing.ambiguity {
		t.Fatalf("foreign nonce accepted: %v", err)
	}
	if len(fake.sentKind("/dev connect")) != 0 {
		t.Fatal("connected on an uncorrelated reset receipt")
	}
}
