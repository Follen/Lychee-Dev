package live

import (
	"context"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"image"
	"testing"
)

func TestBindWindowArchivesOnlyObservedSession(t *testing.T) {
	for _, mode := range []string{"valid", "discovery", "whole-window", "snapshot-mismatch", "wrong-nonce", "missing-pin", "invalid-region"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, client, _, prepared, input := probeOperationFixture(t)
			template, _ := operationSessionFixture(t, client, input)
			request := WindowBindingRequest{Installation: client, PID: template.target.Window.ProcessID, Snapshot: prepared.Intent.Snapshot, Character: input.Expected.Character, Realm: input.Expected.Realm, Nonce: input.Expected.SessionNonce, Region: image.Rect(0, 0, 600, 600)}
			if mode == "missing-pin" {
				request.Snapshot = "PIN-missing"
			}
			if mode == "invalid-region" {
				request.Region = image.Rect(0, 0, 5000, 5000)
			}
			if mode == "whole-window" {
				request.Region = image.Rectangle{}
			}
			if mode == "discovery" {
				request = WindowBindingRequest{Snapshot: request.Snapshot}
			}
			resolved, opened := 0, 0
			var frames *sessionFixtureFrames
			resolve := func(context.Context, string, uint32) (ClientWindow, error) {
				resolved++
				target := template.target
				if mode == "snapshot-mismatch" {
					target.Client.Product = "classic"
				}
				return target, nil
			}
			open := func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
				opened++
				if region != request.Region || expected.Release != buildinfo.Version || expected.SessionNonce != request.Nonce || !expected.RequireInputReady {
					t.Fatal("wrong native observation request")
				}
				signal := template.ready
				if mode == "wrong-nonce" {
					signal.SessionNonce = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
				}
				frames = &sessionFixtureFrames{ackFrames: ackFrames{frame: makeAckFrame(t, signal)}}
				session, err := observeWindowSession(ctx, target, expected, frames, func(context.Context, ClientWindow) error { return nil })
				if err == nil {
					session.region = region
				}
				return session, err
			}
			bound, err := bindWindowSession(ctx, root, request, resolve, open)
			if mode == "valid" || mode == "whole-window" || mode == "discovery" {
				if err != nil || bound.Ready != template.ready || bound.Target != template.target || bound.Record.Snapshot != prepared.Intent.Snapshot || bound.Record.Region != request.Region || !frames.closed {
					t.Fatalf("bound: %+v %v", bound, err)
				}
				saved, err := ReadWindowSession(ctx, root, bound.Record.ID)
				if err != nil || saved != bound {
					t.Fatalf("saved: %+v %v", saved, err)
				}
			} else if err == nil || bound.Record.ID != "" {
				t.Fatalf("accepted %s: %+v %v", mode, bound, err)
			}
			if (mode == "missing-pin" || mode == "invalid-region") && resolved != 0 {
				t.Fatal("resolved window before request/pin validation")
			}
			if mode == "snapshot-mismatch" && opened != 0 {
				t.Fatal("captured wrong snapshot client")
			}
			if frames != nil && !frames.closed {
				t.Fatal("capture stream leaked")
			}
		})
	}
}
