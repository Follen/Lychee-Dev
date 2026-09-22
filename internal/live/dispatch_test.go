package live

import (
	"context"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"hash/adler32"
	"sync"
	"testing"
)

func TestDispatchRequiresReadinessAndSingleDurableIntent(t *testing.T) {
	for _, ready := range []bool{false, true} {
		t.Run(fmt.Sprint(ready), func(t *testing.T) {
			ctx := context.Background()
			root, _, book, record, input := probeOperationFixture(t)
			if command, err := RequestOperationDispatch(ctx, root, record.OperationID); !errors.Is(err, journal.ErrTransition) || command != "" {
				t.Fatalf("premature dispatch: %q %v", command, err)
			}
			if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
				t.Fatal(err)
			}
			e := input.Expected
			loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Release: e.Release, Kind: "loaded", SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: ready, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
			if _, err := ObserveOperationLoaded(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, loaded)})); err != nil {
				t.Fatal(err)
			}
			var wg sync.WaitGroup
			commands := make(chan string, 2)
			failures := make(chan error, 2)
			for i := 0; i < 2; i++ {
				wg.Add(1)
				go func() {
					defer wg.Done()
					command, err := RequestOperationDispatch(ctx, root, record.OperationID)
					commands <- command
					failures <- err
				}()
			}
			wg.Wait()
			close(commands)
			close(failures)
			successes := 0
			for command := range commands {
				if command != "" {
					successes++
					if command != "/dev bridge run "+e.RequestID {
						t.Fatal(command)
					}
				}
			}
			for err := range failures {
				if !ready && (err == nil || err.Error() != "bridge.input_not_ready") {
					t.Fatalf("readiness: %v", err)
				}
				if ready && err != nil && !errors.Is(err, journal.ErrTransition) {
					t.Fatal(err)
				}
			}
			current, err := book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if !ready {
				if successes != 0 || current.Stage != "loaded" {
					t.Fatalf("unready dispatched: %d %+v", successes, current)
				}
				if _, err := ObserveOperationLoaded(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, loaded)})); err == nil {
					t.Fatal("old loaded signal accepted as refresh")
				}
				loaded.Sequence++
				loaded.InputReady = true
				if _, err := ObserveOperationLoaded(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, loaded)})); err != nil {
					t.Fatal(err)
				}
				if command, err := RequestOperationDispatch(ctx, root, record.OperationID); err != nil || command != "/dev bridge run "+e.RequestID {
					t.Fatalf("refreshed dispatch: %q %v", command, err)
				}
				return
			}
			if successes != 1 || current.Stage != "dispatch_requested" {
				t.Fatalf("dispatch intent: %d %+v", successes, current)
			}
			if command, err := RequestOperationDispatch(ctx, root, record.OperationID); err == nil || command != "" {
				t.Fatal("dispatch replayed")
			}
			reported := loaded
			reported.Kind, reported.ReloadNonce, reported.InputReady, reported.Sequence = "reported", "", false, 3
			reported.ReportBytes, reported.ReportAdler32 = 2, "017500f9"
			for _, mode := range []string{"stale", "wrong-code", "foreign", "reload", "missing"} {
				candidate := reported
				feed := &ackFrames{}
				switch mode {
				case "stale":
					candidate.Sequence = 2
				case "wrong-code":
					candidate.CodeAdler32 = "00000000"
				case "foreign":
					candidate.SessionNonce = "other"
				case "reload":
					candidate.ReloadNonce = input.Load.ReloadNonce
				}
				if mode != "missing" {
					feed.frame = makeAckFrame(t, candidate)
				}
				if capture, err := ObserveOperationReported(ctx, root, record.OperationID, bridge.ObserveSignals(feed)); err == nil || capture.ID != "" {
					t.Fatalf("accepted %s: %+v %v", mode, capture, err)
				}
			}
			capture, err := ObserveOperationReported(ctx, root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, reported)}))
			if err != nil || capture.ID == "" {
				t.Fatalf("reported: %+v %v", capture, err)
			}
			current, err = book.InspectWork(ctx, record.OperationID)
			if err != nil || current.Stage != "reported" {
				t.Fatalf("receipt claimed persisted/verified: %+v %v", current, err)
			}
		})
	}
}
