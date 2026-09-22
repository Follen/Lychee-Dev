package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"testing"
)

func TestBootstrapInputFailureDoesNotReplay(t *testing.T) {
	for _, mode := range []string{"partial", "cancelled", "unready", "runtime", "guid"} {
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
			ready := session.ready
			switch mode {
			case "unready":
				ready.InputReady = false
			case "runtime":
				ready.RuntimeEpoch++
			case "guid":
				ready.GUID = "Player-1-other"
			}
			frames.frame = makeAckFrame(t, ready)
			frames.frame.SystemTicks = 2
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
				if command != "/dev bridge prepare "+input.Expected.RequestID+" "+input.Load.ReloadNonce {
					t.Fatal(command)
				}
				current, err := book.InspectWork(ctx, record.OperationID)
				var observed probeLoadObservation
				if err != nil || json.Unmarshal(current.Observation, &observed) != nil || observed.BootstrapReadyID == "" {
					t.Fatal("input before durable intent", err)
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if mode == "cancelled" {
					cancel()
					return desktop.InputReceipt{MessagesQueued: 2}, ctx.Err()
				}
				return desktop.InputReceipt{MessagesQueued: 2}, errors.New("fixture.partial_bootstrap")
			}
			receipt, err := operation.bootstrap(ctx, send)
			if err == nil {
				t.Fatal("failed bootstrap accepted")
			}
			current, err := book.InspectWork(context.Background(), record.OperationID)
			var observed probeLoadObservation
			if err != nil || json.Unmarshal(current.Observation, &observed) != nil {
				t.Fatal(err)
			}
			if current.Stage != "load_requested" || observed.BootstrapCapture != "" {
				t.Fatal("input claimed reentry")
			}
			if mode == "unready" || mode == "runtime" || mode == "guid" {
				if receipt.MessagesQueued != 0 || observed.BootstrapReadyID != "" || observed.BootstrapInput != nil {
					t.Fatal("ineligible bootstrap submitted input")
				}
				return
			}
			if current.Status != "unresolved" || observed.BootstrapInput == nil || *observed.BootstrapInput != receipt {
				t.Fatal("partial outcome missing")
			}
			if _, err := operation.bootstrap(context.Background(), send); err == nil || calls != 1 {
				t.Fatal("bootstrap replayed")
			}
		})
	}
}

func exerciseBootstrap(t *testing.T, p *ProbeOperation, session *WindowSession, frames *sessionFixtureFrames, book *journal.Book, record journal.WorkRecord, input ReportIntent) string {
	t.Helper()
	ctx := context.Background()
	initial := session.ready
	if _, err := p.ObserveBootstrap(ctx); err == nil {
		t.Fatal("bootstrap without intent")
	}
	frames.frame = makeAckFrame(t, initial)
	frames.frame.SystemTicks = 2
	calls := 0
	send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		calls++
		if target != session.target.Window {
			t.Fatal("bootstrap changed window")
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if command != "/dev bridge prepare "+input.Expected.RequestID+" "+input.Load.ReloadNonce {
			t.Fatal(command)
		}
		current, err := book.InspectWork(ctx, record.OperationID)
		var observed probeLoadObservation
		if err != nil || json.Unmarshal(current.Observation, &observed) != nil || observed.BootstrapReadyID == "" || observed.BootstrapCapture != "" || current.Stage != "load_requested" {
			t.Fatal("bootstrap input preceded intent", err)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 42, SubmissionComplete: true}, nil
	}
	inputReceipt, err := p.bootstrap(ctx, send)
	if err != nil {
		t.Fatal(err)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	var observed probeLoadObservation
	if err != nil || json.Unmarshal(current.Observation, &observed) != nil || observed.BootstrapInput == nil || *observed.BootstrapInput != inputReceipt {
		t.Fatal("bootstrap outcome lost", err)
	}
	if _, err := p.bootstrap(ctx, send); err == nil || calls != 1 {
		t.Fatal("bootstrap replayed")
	}
	if _, err := p.Observe(ctx); err == nil {
		t.Fatal("load observation bypassed bootstrap handoff")
	}
	reentry := initial
	reentry.RequestID, reentry.ReloadNonce, reentry.RuntimeEpoch, reentry.Sequence = input.Expected.RequestID, input.Load.ReloadNonce, initial.RuntimeEpoch+1, 1
	for index, mutate := range []func(*bridge.Signal){
		func(s *bridge.Signal) { s.RuntimeEpoch-- },
		func(s *bridge.Signal) { s.RuntimeEpoch++ },
		func(s *bridge.Signal) { s.RequestID = "REQ-other" },
		func(s *bridge.Signal) { s.ReloadNonce = "" },
		func(s *bridge.Signal) { s.GUID = "Player-1-other" },
		func(s *bridge.Signal) { s.InputReady = false },
	} {
		wrong := reentry
		mutate(&wrong)
		frames.frame = makeAckFrame(t, wrong)
		frames.frame.SystemTicks = int64(3 + index)
		if _, err := p.ObserveBootstrap(ctx); err == nil {
			t.Fatal("invalid bootstrap accepted", index)
		}
		if session.ready != initial {
			t.Fatal("invalid bootstrap adopted runtime")
		}
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 10
	capture, err := p.ObserveBootstrap(ctx)
	if err != nil || session.ready != reentry {
		t.Fatal("bootstrap not adopted", err)
	}
	committed, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if committed.Stage != "load_requested" {
		t.Fatal("bootstrap claimed compiled or executed")
	}
	session.ready = initial
	if err := p.check(ctx); err == nil {
		t.Fatal("stale bootstrap runtime accepted")
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 11
	repeated, err := p.ObserveBootstrap(ctx)
	if err != nil || repeated.ID != capture.ID || session.ready != reentry {
		t.Fatal("bootstrap reconciliation", err)
	}
	stable, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || stable.Generation != committed.Generation {
		t.Fatal("bootstrap reconciliation churned generation", err)
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 12
	busy := errors.New("fixture.input_busy")
	if _, err := p.load(ctx, func(context.Context, desktop.WindowIdentity, func(context.Context) (string, error), func(context.Context) error) (desktop.InputReceipt, error) {
		return desktop.InputReceipt{}, busy
	}); !errors.Is(err, busy) {
		t.Fatal("lost input lock error", err)
	}
	unchanged, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || unchanged.Generation != stable.Generation {
		t.Fatal("failed input admission overwrote bootstrap outcome", err)
	}
	loadCalls := 0
	loadSender := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		loadCalls++
		if target != session.target.Window {
			t.Fatal("load changed window")
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if command != "/dev bridge load "+input.Expected.RequestID {
			t.Fatal(command)
		}
		current, err := book.InspectWork(ctx, record.OperationID)
		var observed probeLoadObservation
		if err != nil || json.Unmarshal(current.Observation, &observed) != nil || observed.LoadReadyCapture == "" || observed.LoadedCapture != "" {
			t.Fatal("load before durable intent", err)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 17, SubmissionComplete: true}, nil
	}
	loadReceipt, err := p.load(ctx, loadSender)
	if err != nil {
		t.Fatal("load submission", err)
	}
	current, err = book.InspectWork(ctx, record.OperationID)
	if err != nil || json.Unmarshal(current.Observation, &observed) != nil || observed.LoadInput == nil || *observed.LoadInput != loadReceipt || observed.BootstrapInput == nil || *observed.BootstrapInput != inputReceipt || current.Stage != "load_requested" {
		t.Fatal("load outcome lost or claimed compilation", err)
	}
	if _, err := p.load(ctx, loadSender); err == nil || loadCalls != 1 {
		t.Fatal("load replayed")
	}
	return capture.ID
}
