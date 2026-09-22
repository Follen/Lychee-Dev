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
)

func TestFlushRequiresNewReadinessAndDoesNotReplay(t *testing.T) {
	for _, mode := range []string{"success", "partial", "cancelled", "unready", "stale", "guid", "request", "runtime"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			root, client, book, record, input := probeOperationFixture(t)
			if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: true, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			if _, err := ObserveOperationLoaded(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, loaded)})); err != nil {
				t.Fatal(err)
			}
			if _, err := RequestOperationDispatch(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			reported := loaded
			reported.Kind = "reported"
			reported.ReloadNonce = ""
			reported.InputReady = false
			reported.Sequence = 3
			reported.ReportBytes = 2
			reported.ReportAdler32 = fmt.Sprintf("%08x", adler32.Checksum([]byte("42")))
			if _, err := ObserveOperationReported(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, reported)})); err != nil {
				t.Fatal(err)
			}
			session, frames := operationSessionFixture(t, client, input)
			operation, err := session.OpenOperation(ctx, root, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			defer operation.Close()
			ready := session.ready
			ready.Sequence = 4
			switch mode {
			case "runtime":
				ready.RuntimeEpoch++
			case "unready":
				ready.InputReady = false
			case "stale":
				ready.Sequence = 3
			case "guid":
				ready.GUID = "Player-1-other"
			case "request":
				ready.RequestID = e.RequestID
			}
			frames.frame = makeAckFrame(t, ready)
			frames.frame.SystemTicks = 4
			calls := 0
			send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				calls++
				if target != session.target.Window {
					t.Fatal("wrong window")
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				if command != "/dev bridge reload "+e.RequestID {
					t.Fatal("wrong flush command", command)
				}
				current, err := book.InspectWork(ctx, record.OperationID)
				if err != nil || current.Stage != "flush_requested" {
					t.Fatal("input before flush intent", err)
				}
				var observation probeLoadObservation
				if err := json.Unmarshal(current.Observation, &observation); err != nil || observation.FlushReadyCapture == "" {
					t.Fatal("missing readiness evidence", err)
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if mode == "partial" {
					return desktop.InputReceipt{MessagesQueued: 2}, errors.New("fixture.partial_reload")
				}
				if mode == "cancelled" {
					cancel()
					return desktop.InputReceipt{MessagesQueued: 2}, ctx.Err()
				}
				return desktop.InputReceipt{MessagesQueued: 11, SubmissionComplete: true}, nil
			}
			receipt, err := operation.flush(ctx, send)
			if (err == nil) != (mode == "success") {
				t.Fatalf("flush %+v %v", receipt, err)
			}
			current, err := book.InspectWork(context.Background(), record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if mode == "unready" || mode == "stale" || mode == "guid" || mode == "request" || mode == "runtime" {
				if current.Stage != "reported" || receipt.MessagesQueued != 0 {
					t.Fatal("ineligible flush mutated stage")
				}
				return
			}
			want := "running"
			if mode != "success" {
				want = "unresolved"
			}
			if current.Stage != "flush_requested" || current.Status != want {
				t.Fatalf("input claimed persistence: %+v", current)
			}
			var observed probeLoadObservation
			if err := json.Unmarshal(current.Observation, &observed); err != nil || observed.FlushInput == nil || *observed.FlushInput != receipt {
				t.Fatal("flush outcome missing", err)
			}
			if _, err := operation.flush(context.Background(), send); err == nil || calls != 1 {
				t.Fatal("flush replayed")
			}
		})
	}
}
