package live

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
)

// Exercise the actual reload transaction with old runtime, current managed
// disk bytes, and a correlated current-release receipt after refresh.
func TestReloadActivatesManagedUpgrade(t *testing.T) {
	for _, mode := range []string{"upgrade", "changed-disk", "wrong-release", "wrong-actor", "wrong-nonce"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, client, _, pin, template, _ := unpreparedProbeFixture(t, 7)
			initial := template.ready
			initial.Release = "2.0.3"
			template.Close()
			frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
			session, err := observeWindowSession(ctx, template.target, initialExpectation(initial), frames, func(context.Context, ClientWindow) error { return nil })
			if err != nil {
				t.Fatal(err)
			}
			defer session.Close()
			record, err := session.prepareStandaloneReload(ctx, root, pin.ID, "managed-upgrade")
			if err != nil {
				t.Fatal(err)
			}
			if mode == "changed-disk" {
				if err := os.WriteFile(filepath.Join(client, "Interface", "AddOns", "Lychee Dev", "unexpected.lua"), []byte("return 1"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			op, err := session.openStandaloneReload(ctx, root, record.OperationID)
			if mode == "changed-disk" {
				if err == nil {
					op.close()
					t.Fatal("upgrade accepted changed managed bytes")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			defer op.close()
			sent := 0
			completed, err := op.execute(ctx, func(ctx context.Context, _ desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if _, err := prepare(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				sent++
				intent, _ := parseStandaloneReload(record)
				next := initial
				next.Release = buildinfo.Version
				next.RequestID, next.ReloadNonce = intent.Expected.RequestID, intent.ReloadNonce
				next.RuntimeEpoch, next.Sequence = initial.RuntimeEpoch+1, 1
				switch mode {
				case "wrong-release":
					next.Release = "9.9.9"
				case "wrong-actor":
					next.GUID = "Player-1-other"
				case "wrong-nonce":
					next.ReloadNonce = "ffffffffffffffffffffffffffffffff"
				}
				frames.signals = append(frames.signals, next)
				return desktop.InputReceipt{MessagesQueued: 3, SubmissionComplete: true}, nil
			})
			if mode != "upgrade" {
				if err == nil {
					t.Fatal("accepted unrelated upgrade receipt")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			result, err := Status(ctx, root, completed.OperationID)
			if err != nil || !result.Complete || sent != 1 {
				t.Fatalf("upgrade=%+v sent=%d err=%v", result, sent, err)
			}
			intent, _ := parseStandaloneReload(record)
			// The public API must find the completed request before trying any
			// native input (the fake fixture has no native window).
			repeated, err := ReloadClient(ctx, root, ReloadRequest{Session: intent.Binding, Request: "managed-upgrade"})
			if err != nil || !repeated.Complete || repeated.OperationID != record.OperationID {
				t.Fatalf("public upgrade retry: %+v %v", repeated, err)
			}
		})
	}
}

func TestReloadReconnectBeforeAndAfterManagedUpgrade(t *testing.T) {
	for _, activeRelease := range []string{"2.0.3", buildinfo.Version} {
		t.Run(activeRelease, func(t *testing.T) {
			ctx := context.Background()
			root, _, _, pin, original, _ := unpreparedProbeFixture(t, 9)
			original.ready.Release = "2.0.3"
			saved, err := SaveWindowSession(ctx, root, pin.ID, original)
			if err != nil {
				t.Fatal(err)
			}
			defer original.Close()
			fake := newFakeIO(t)
			handle := original.target.Window.Handle
			fake.respond[handle] = func(command string) []*desktop.CapturedFrame {
				if command == "/dev connect" {
					ready := original.ready
					ready.Release = activeRelease
					ready.RuntimeEpoch++
					ready.Sequence = 1
					return []*desktop.CapturedFrame{fake.frame(handle, ready), fake.frame(handle, ready)}
				}
				if !strings.HasPrefix(command, "/dev bridge identify ") {
					t.Fatalf("unexpected command %q", command)
				}
				identity := identityReceipt(nonceFromCommand(command), func(s *bridge.Signal) {
					s.Release = activeRelease
					s.Character, s.Realm, s.GUID = original.ready.Character, original.ready.Realm, original.ready.GUID
					s.Product, s.Build = original.ready.Product, original.ready.Build
				})
				return []*desktop.CapturedFrame{fake.frame(handle, identity)}
			}
			session, snapshot, err := reconnectForReload(ctx, root, saved.ID, fake.io())
			if err != nil {
				t.Fatal(err)
			}
			defer session.Close()
			if session.ready.Release != activeRelease || session.target != original.target || snapshot != pin.ID || len(fake.sentKind("/dev connect")) != 1 {
				t.Fatalf("wrong reload reconnect: %+v commands=%v", session, fake.commands())
			}
		})
	}
}
