package live

import (
	"context"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestActionReconnectAfterStandaloneReload(t *testing.T) {
	for _, mode := range []string{"reloaded", "different-actor", "combat", "busy", "changed-process", "older-runtime"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, _, _, pin, original, _ := unpreparedProbeFixture(t, 9)
			saved, err := SaveWindowSession(ctx, root, pin.ID, original)
			if err != nil {
				t.Fatal(err)
			}
			defer original.Close()
			fake := newFakeIO(t)
			handle := original.target.Window.Handle
			reloaded := original.ready
			reloaded.RuntimeEpoch++
			reloaded.Sequence = 1
			reloaded.RequestID = "RELOAD-0123456789abcdef0123456789abcdef"
			reloaded.ReloadNonce = "0123456789abcdef0123456789abcdef"
			fake.display(handle).push(fake.frame(handle, reloaded))
			if mode == "busy" {
				fake.owners[windowResource(original.target)] = journal.WindowOwner{OperationID: "OP-other"}
			}
			if mode == "changed-process" {
				fake.confirmErr[handle] = desktop.ErrIdentityChanged
			}
			fake.respond[handle] = func(command string) []*desktop.CapturedFrame {
				if command == "/dev connect" {
					ready := reloaded
					ready.RequestID, ready.ReloadNonce = "", ""
					ready.Sequence++
					if mode == "older-runtime" {
						ready.RuntimeEpoch = 0
					}
					return []*desktop.CapturedFrame{fake.frame(handle, ready), fake.frame(handle, ready)}
				}
				identity := identityReceipt(nonceFromCommand(command), func(s *bridge.Signal) {
					s.Character, s.Realm, s.GUID = original.ready.Character, original.ready.Realm, original.ready.GUID
					s.Product, s.Build = original.ready.Product, original.ready.Build
					if mode == "different-actor" {
						s.GUID = "Player-1-other"
					}
					if mode == "combat" {
						s.InputReady, s.InputReason = false, "input_combat_lockdown"
					}
				})
				return []*desktop.CapturedFrame{fake.frame(handle, identity)}
			}
			session, snapshot, err := reconnectSession(ctx, root, saved.ID, fake.io())
			if mode != "reloaded" {
				if err == nil || session != nil {
					t.Fatalf("unsafe reconnect %s: %+v %v", mode, session, err)
				}
				if mode != "older-runtime" && len(fake.sentKind("/dev connect")) != 0 {
					t.Fatal("connected without identity/readiness")
				}
				if (mode == "busy" || mode == "changed-process") && len(fake.commands()) != 0 {
					t.Fatal("input into unavailable window")
				}
				return
			}
			if err != nil {
				t.Fatalf("next action after reload cannot reconnect: %v", err)
			}
			defer session.Close()
			if snapshot != pin.ID || session.target != original.target || session.ready.RequestID != "" || session.ready.RuntimeEpoch != reloaded.RuntimeEpoch {
				t.Fatalf("wrong session: %+v", session)
			}
			if len(fake.sentKind("/dev connect")) != 1 {
				t.Fatal(fake.commands())
			}
			for _, command := range fake.commands() {
				if strings.Contains(command, "refresh") || strings.Contains(command, "/reload") {
					t.Fatal("replayed reload")
				}
			}
		})
	}
}
