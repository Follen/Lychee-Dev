package live

import (
	"context"
	"errors"
	"image"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func connectFixture(t *testing.T) (string, *fakeIO, string) {
	t.Helper()
	gameRoot := t.TempDir()
	client := makeTestClient(t, gameRoot, "_retail_", "wow", "12.1.0.69875")
	root := filepath.Join(gameRoot, "home")
	if err := os.MkdirAll(root, 0700); err != nil {
		t.Fatal(err)
	}
	fake := newFakeIO(t)
	return client, fake, root
}

func TestConnectFirstContactBootstrapsAndPersists(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if command == "/dev connect" {
			return []*desktop.CapturedFrame{
				fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence = false, 1 })),
				fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence = true, 2 })),
			}
		}
		return identityResponder(fake, 1, nil)(command)
	}
	connection, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	if connection.Character != "Paladin" || connection.Realm != "Realm" || connection.ID == "" || connection.CaptureID == "" || connection.Target.Window.Handle != 1 {
		t.Fatalf("connection: %+v", connection)
	}
	commands := fake.commands()
	if len(commands) != 3 || !strings.Contains(commands[0], ":/dev bridge identify ") || !strings.Contains(commands[1], ":/dev bridge identify ") || commands[2] != "1:/dev connect" {
		t.Fatalf("input sequence: %v", commands)
	}
	if nonce := nonceFromCommand(strings.TrimPrefix(commands[0], "1:")); len(nonce) != 32 || strings.Trim(nonce, "0123456789abcdef") != "" {
		t.Fatalf("probe nonce: %q", nonce)
	}
	saved, err := ReadWindowSession(context.Background(), root, connection.ID)
	if err != nil || saved.Ready.Character != "Paladin" || saved.Ready.Kind != "ready" || saved.Connection() != connection {
		t.Fatalf("persisted session: %+v %v", saved, err)
	}
}

func TestConnectInputReadyGateNeverSendsConnect(t *testing.T) {
	for _, mode := range []string{"combat", "focus-never-released"} {
		t.Run(mode, func(t *testing.T) {
			client, fake, root := connectFixture(t)
			fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
			pin := testPin(t, root)
			fake.respond[1] = func(command string) []*desktop.CapturedFrame {
				nonce := nonceFromCommand(command)
				reason := "input_combat_lockdown"
				if mode == "focus-never-released" {
					reason = inputKeyboardFocus
				}
				return []*desktop.CapturedFrame{fake.frame(1, identityReceipt(nonce, func(signal *bridge.Signal) {
					signal.InputReady, signal.InputReason = false, reason
				}))}
			}
			connection, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
			var notReady *InputNotReadyError
			if err == nil || connection.ID != "" || !errors.As(err, &notReady) {
				t.Fatalf("gate: %+v %v", connection, err)
			}
			want := "input_combat_lockdown"
			if mode == "focus-never-released" {
				want = inputKeyboardFocus
			}
			if notReady.Reason != want {
				t.Fatalf("reason %q, want %q", notReady.Reason, want)
			}
			for _, command := range fake.commands() {
				if command == "1:/dev connect" {
					t.Fatal("sent /dev connect without verified input readiness")
				}
			}
			// One identity trigger for discovery and one for the pre-effect
			// re-verification; the readiness gate stops before /dev connect.
			if len(fake.commands()) != 2 {
				t.Fatalf("unexpected input: %v", fake.commands())
			}
		})
	}
}

func TestConnectSelectionHonorsConstraintsWithoutGuessing(t *testing.T) {
	for _, mode := range []string{"ambiguous", "unique-pid", "unique-character", "missing", "ambiguous-identical"} {
		t.Run(mode, func(t *testing.T) {
			client, fake, root := connectFixture(t)
			fake.windows = []desktop.WindowIdentity{testWindow(1, client), testWindow(2, client)}
			second := func(signal *bridge.Signal) {
				if mode == "ambiguous-identical" {
					return
				}
				signal.Character, signal.GUID = "Warlock", "Player-1-456"
			}
			for _, handle := range []uint64{1, 2} {
				handle := handle
				mutate := second
				if handle == 1 {
					mutate = nil
				}
				fake.respond[handle] = func(command string) []*desktop.CapturedFrame {
					if command == "/dev connect" {
						character, guid := "Paladin", "Player-1-123"
						if handle == 2 && mode != "ambiguous-identical" {
							character, guid = "Warlock", "Player-1-456"
						}
						return []*desktop.CapturedFrame{fake.frame(handle, readyReceipt(func(signal *bridge.Signal) {
							signal.Character, signal.GUID, signal.InputReady, signal.Sequence = character, guid, true, 2
						}))}
					}
					return identityResponder(fake, handle, mutate)(command)
				}
			}
			request := ConnectRequest{Snapshot: testPin(t, root)}
			switch mode {
			case "unique-pid":
				request.PID = fake.windows[1].ProcessID
			case "unique-character":
				request.Character = "Warlock"
			case "missing":
				request.Character = "Nobody"
			}
			connection, err := connectWindow(context.Background(), root, request, fake.io())
			switch mode {
			case "ambiguous", "ambiguous-identical":
				var choice *CandidateSelectionError
				if !errors.Is(err, ErrCandidateAmbiguous) || !errors.As(err, &choice) || len(choice.Candidates) != 2 {
					t.Fatalf("ambiguity: %+v %v", choice, err)
				}
				if connection.ID != "" {
					t.Fatal("ambiguous connect produced a session")
				}
			case "missing":
				var choice *CandidateSelectionError
				if !errors.Is(err, ErrCandidateMissing) || !errors.As(err, &choice) || len(choice.Candidates) != 2 {
					t.Fatalf("missing: %+v %v", choice, err)
				}
			default:
				if err != nil {
					t.Fatal(err)
				}
				if mode == "unique-pid" && connection.Target.Window.ProcessID != fake.windows[1].ProcessID {
					t.Fatalf("wrong window: %+v", connection)
				}
				if mode == "unique-character" && connection.Character != "Warlock" {
					t.Fatalf("wrong actor: %+v", connection)
				}
			}
		})
	}
}

func TestConnectReverifyBeforeEffect(t *testing.T) {
	for _, mode := range []string{"actor-switched", "window-reused"} {
		t.Run(mode, func(t *testing.T) {
			client, fake, root := connectFixture(t)
			fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
			pin := testPin(t, root)
			identifies := 0
			fake.respond[1] = func(command string) []*desktop.CapturedFrame {
				if command == "/dev connect" {
					t.Fatal("connected before re-verification completed")
				}
				identifies++
				nonce := nonceFromCommand(command)
				mutate := func(signal *bridge.Signal) {}
				if mode == "actor-switched" && identifies > 1 {
					mutate = func(signal *bridge.Signal) { signal.Character, signal.GUID = "Rogue", "Player-1-999" }
				}
				unready := identityReceipt(nonce, func(signal *bridge.Signal) {
					signal.InputReady, signal.InputReason = false, inputKeyboardFocus
					mutate(signal)
				})
				refreshed := identityReceipt(nonce, mutate)
				return []*desktop.CapturedFrame{fake.frame(1, unready), fake.frame(1, refreshed)}
			}
			if mode == "window-reused" {
				io := fake.io()
				io.confirm = func(context.Context, ClientWindow) error { return desktop.ErrIdentityChanged }
				_, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, io)
				if !errors.Is(err, desktop.ErrIdentityChanged) {
					t.Fatalf("reused handle: %v", err)
				}
				for _, command := range fake.commands() {
					if command == "1:/dev connect" {
						t.Fatal("connected to a reused handle")
					}
				}
				if len(fake.commands()) != 1 {
					t.Fatalf("unexpected input: %v", fake.commands())
				}
				return
			}
			_, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
			if !errors.Is(err, ErrActorChanged) {
				t.Fatalf("actor switch: %v", err)
			}
			for _, command := range fake.commands() {
				if command == "1:/dev connect" {
					t.Fatal("connected after actor switch")
				}
			}
			if len(fake.commands()) != 2 {
				t.Fatalf("unexpected input: %v", fake.commands())
			}
		})
	}
}

func TestConnectSessionReuseAndRevive(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	guid := "Player-1-123"
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if command == "/dev connect" {
			return []*desktop.CapturedFrame{fake.frame(1, readyReceipt(func(signal *bridge.Signal) {
				signal.GUID, signal.InputReady, signal.Sequence = guid, true, 2
			}))}
		}
		return []*desktop.CapturedFrame{fake.frame(1, identityReceipt(nonceFromCommand(command), func(signal *bridge.Signal) {
			signal.GUID = guid
		}))}
	}
	first, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	sentAfterFirst := len(fake.commands())
	// A valid session is returned as-is: one identity re-verify, no re-opt-in.
	again, err := connectWindow(context.Background(), root, ConnectRequest{Session: first.ID}, fake.io())
	if err != nil || again != first {
		t.Fatalf("reuse: %+v %v", again, err)
	}
	for _, command := range fake.commands()[sentAfterFirst:] {
		if command == "1:/dev connect" {
			t.Fatal("valid session re-opted-in")
		}
	}
	if len(fake.commands())-sentAfterFirst != 1 {
		t.Fatalf("unexpected re-verify input: %v", fake.commands()[sentAfterFirst:])
	}
	// A stale session reconnects through the same flow and keeps the old record.
	sentBeforeStale := len(fake.commands())
	guid = "Player-1-777"
	revived, err := connectWindow(context.Background(), root, ConnectRequest{Session: first.ID}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	if revived.ID == first.ID || revived.Character != "Paladin" {
		t.Fatalf("revive: %+v", revived)
	}
	connects := 0
	for _, command := range fake.commands()[sentBeforeStale:] {
		if command == "1:/dev connect" {
			connects++
		}
	}
	if connects != 1 {
		t.Fatalf("stale revive input: %v", fake.commands()[sentBeforeStale:])
	}
	history, err := ReadWindowSession(context.Background(), root, first.ID)
	if err != nil || history.Ready.GUID != "Player-1-123" {
		t.Fatalf("old record erased: %+v %v", history, err)
	}
	if _, err := ReadWindowSession(context.Background(), root, revived.ID); err != nil {
		t.Fatal(err)
	}
}

func TestConnectReviveRejectsConstraintOverrides(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if command == "/dev connect" {
			return []*desktop.CapturedFrame{fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence = true, 2 }))}
		}
		return identityResponder(fake, 1, nil)(command)
	}
	first, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	for _, override := range []ConnectRequest{
		{Session: first.ID, Character: "Other"},
		{Session: first.ID, Realm: "Elsewhere"},
		{Session: first.ID, PID: 999},
		{Session: first.ID, Installation: filepath.Dir(client)},
		{Session: first.ID, Snapshot: "PIN-other"},
	} {
		if _, err := connectWindow(context.Background(), root, override, fake.io()); !errors.Is(err, ErrSessionConstraint) {
			t.Fatalf("override %+v accepted: %v", override, err)
		}
	}
}

func TestConnectNeverTypesIntoBusyWindows(t *testing.T) {
	t.Run("busy-from-start", func(t *testing.T) {
		client, fake, root := connectFixture(t)
		window := testWindow(1, client)
		fake.windows = []desktop.WindowIdentity{window}
		pin := testPin(t, root)
		resource := windowHandleResource(window)
		fake.owners[resource] = journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("d", 32), Resource: resource, OperationID: "OP-" + strings.Repeat("e", 32)}
		_, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
		var choice *CandidateSelectionError
		if !errors.Is(err, ErrCandidateMissing) || !errors.As(err, &choice) || len(choice.Candidates) != 1 || choice.Candidates[0].State != CandidateBusy || !choice.Candidates[0].ForeignOwner {
			t.Fatalf("busy candidate: %+v %v", choice, err)
		}
		if len(fake.commands()) != 0 {
			t.Fatalf("typed into a busy window: %v", fake.commands())
		}
	})
	t.Run("busy-before-effect", func(t *testing.T) {
		client, fake, root := connectFixture(t)
		window := testWindow(1, client)
		fake.windows = []desktop.WindowIdentity{window}
		pin := testPin(t, root)
		resource := windowHandleResource(window)
		fake.respond[1] = func(command string) []*desktop.CapturedFrame {
			// Another operation claims the window between discovery and effect.
			fake.owners[resource] = journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: strings.Repeat("d", 32), Resource: resource, OperationID: "OP-" + strings.Repeat("e", 32)}
			return identityResponder(fake, 1, nil)(command)
		}
		_, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
		var occupied *journal.WindowOccupied
		if !errors.Is(err, journal.ErrBusy) || !errors.As(err, &occupied) || !occupied.Foreign {
			t.Fatalf("late busy: %v", err)
		}
		for _, command := range fake.commands() {
			if command == "1:/dev connect" {
				t.Fatal("connected while another operation owns the window")
			}
		}
	})
}

func TestConnectIdentityUnreadableIsAPreciseFailure(t *testing.T) {
	client, fake, root := connectFixture(t)
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	pin := testPin(t, root)
	fake.respond[1] = func(string) []*desktop.CapturedFrame { return nil }
	_, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: pin}, fake.io())
	var choice *CandidateSelectionError
	if !errors.Is(err, ErrCandidateMissing) || !errors.As(err, &choice) || len(choice.Candidates) != 1 || choice.Candidates[0].State != CandidateUnreadable {
		t.Fatalf("unreadable candidate: %+v %v", choice, err)
	}
	if len(fake.commands()) != 1 {
		t.Fatalf("unexpected input: %v", fake.commands())
	}
}

func TestConnectRequestValidation(t *testing.T) {
	if err := (ConnectRequest{}).Validate(); err == nil {
		t.Fatal("accepted empty request")
	}
	if err := (ConnectRequest{Snapshot: "PIN-x", CaptureArea: image.Rect(0, 0, 5000, 5000)}).Validate(); err == nil {
		t.Fatal("accepted oversized capture area")
	}
	if err := (ConnectRequest{Snapshot: "PIN-x", Character: "bad\nname"}).Validate(); err == nil {
		t.Fatal("accepted invalid character constraint")
	}
}
