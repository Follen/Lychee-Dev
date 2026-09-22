package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"hash/adler32"
	"testing"
	"time"
)

func TestProbeDispatchPersistsIntentBeforeInputAndNeverReplays(t *testing.T) {
	for _, mode := range []string{"success", "partial", "cancelled", "unready", "busy", "expired"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			root, client, book, record, input := probeOperationFixture(t)
			session, frames := operationSessionFixture(t, client, input)
			operation, err := session.OpenOperation(ctx, root, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			defer operation.Close()
			if _, err := operation.PrepareFiles(ctx); err != nil {
				t.Fatal(err)
			}
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: mode != "unready", CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			frames.frame = makeAckFrame(t, loaded)
			frames.frame.SystemTicks = 2
			calls := 0
			failure := errors.New("fixture.partial_input")
			send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				calls++
				if target != session.target.Window {
					t.Fatal("wrong window")
				}
				if mode == "busy" {
					return desktop.InputReceipt{}, errors.New("desktop.input_busy")
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				if command != "/dev bridge run "+e.RequestID {
					t.Fatal(command)
				}
				current, err := book.InspectWork(ctx, record.OperationID)
				if err != nil || current.Stage != "dispatch_requested" {
					t.Fatal("input preceded durable intent", err)
				}
				if mode == "expired" {
					time.Sleep(1100 * time.Millisecond)
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if mode == "cancelled" {
					cancel()
					return desktop.InputReceipt{MessagesQueued: 2}, ctx.Err()
				}
				if mode == "partial" {
					return desktop.InputReceipt{MessagesQueued: 3}, failure
				}
				return desktop.InputReceipt{MessagesQueued: 42, SubmissionComplete: true}, guard(ctx)
			}
			receipt, err := operation.dispatch(ctx, send)
			if mode == "expired" && (err == nil || err.Error() != "bridge.signal_expired" || receipt.MessagesQueued != 0) {
				t.Fatalf("expired readiness sent input: %+v %v", receipt, err)
			}
			if (err == nil) != (mode == "success") {
				t.Fatalf("result %+v %v", receipt, err)
			}
			current, err := book.InspectWork(context.Background(), record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if mode == "busy" || mode == "unready" {
				if current.Stage == "dispatch_requested" {
					t.Fatal("ineligible input recorded dispatch")
				}
				return
			}
			want := "running"
			if mode != "success" {
				want = "unresolved"
			}
			if current.Stage != "dispatch_requested" || current.Status != want {
				t.Fatalf("input outcome: %+v", current)
			}
			var observed probeLoadObservation
			if err := json.Unmarshal(current.Observation, &observed); err != nil || observed.DispatchInput == nil || *observed.DispatchInput != receipt {
				t.Fatal("input outcome not retained", err)
			}
			if _, err := operation.dispatch(context.Background(), send); err == nil || calls != 1 {
				t.Fatal("dispatch repeated")
			}
		})
	}
}

func TestDispatchPreviouslyObservedLoadedRequiresFreshUnchangedReceipt(t *testing.T) {
	for _, mode := range []string{"fresh", "stale-frame", "changed-receipt"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, client, book, record, input := probeOperationFixture(t)
			session, frames := operationSessionFixture(t, client, input)
			operation, err := session.OpenOperation(ctx, root, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			defer operation.Close()
			if _, err := operation.PrepareFiles(ctx); err != nil {
				t.Fatal(err)
			}
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: true, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			frames.frame = makeAckFrame(t, loaded)
			frames.frame.SystemTicks = 2
			if _, err := operation.Observe(ctx); err != nil {
				t.Fatal(err)
			}
			if mode == "changed-receipt" {
				loaded.InputReady = false
			}
			frames.frame = makeAckFrame(t, loaded)
			frames.frame.SystemTicks = 3
			if mode == "stale-frame" {
				frames.frame.SystemTicks = 2
			}
			submitted := false
			_, err = operation.dispatch(ctx, func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				if command != "/dev bridge run "+e.RequestID {
					t.Fatal(command)
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				submitted = true
				return desktop.InputReceipt{MessagesQueued: 10, SubmissionComplete: true}, nil
			})
			if (err == nil) != (mode == "fresh") || submitted != (mode == "fresh") {
				t.Fatal(mode, submitted, err)
			}
			if mode == "changed-receipt" && err.Error() != "live.loaded_sequence_reused" {
				t.Fatal(err)
			}
			current, err := book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			want := "loaded"
			if mode == "fresh" {
				want = "dispatch_requested"
			}
			if current.Stage != want {
				t.Fatal(current.Stage)
			}
		})
	}
}
