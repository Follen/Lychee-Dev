package live

import (
	"context"
	"encoding/json"
	"fmt"
	"hash/adler32"
	"image"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestFaultsStopsVerifiedThenAcknowledgesWithoutCleanupReload(t *testing.T) {
	ctx := context.Background()
	root, client, book, pin, template, _ := unpreparedProbeFixture(t, 7)
	initial := template.ready
	template.Close()
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
	session, err := observeWindowSession(ctx, template.target, initialExpectation(initial), frames, func(context.Context, ClientWindow) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	record, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "bugs-test-1", Count: 2})
	if err != nil {
		t.Fatal(err)
	}
	repeated, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "bugs-test-1", Count: 2})
	if err != nil || repeated.OperationID != record.OperationID {
		t.Fatalf("idempotent bugs request: %+v %v", repeated, err)
	}
	if _, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "bugs-test-1", Count: 3}); err == nil {
		t.Fatal("changed count reused an idempotency key")
	}
	op, err := session.openFaultOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer op.close()
	input, _, _ := faultInput(record)
	body := `{"schema":"lycheedev.bugs.v1","status":"completed","complete":true,"snapshot":{"returnedCount":2}}`
	reported := bridge.Signal{Schema: "lycheedev.signal.v1", Release: initial.Release, Kind: "reported", SessionNonce: initial.SessionNonce,
		RequestID: input.Expected.RequestID, Character: initial.Character, Realm: initial.Realm, Product: initial.Product, Build: initial.Build,
		Sequence: initial.Sequence + 1, ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	reportedRaw, _ := json.Marshal(reported)
	var reentry bridge.Signal
	var commands []string
	send := func(ctx context.Context, _ desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if len(commands) == 2 {
			// The reentry receipt stays displayed after the reload, so the ack
			// preparation observes a fresh frame of it before queueing input.
			frames.signals = append(frames.signals, reentry)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		commands = append(commands, command)
		switch {
		case strings.HasPrefix(command, "/dev bridge bugs "):
			frames.signals = append(frames.signals, reported)
		case strings.HasPrefix(command, "/dev bridge flush "):
			source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
			if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
				return desktop.InputReceipt{}, err
			}
			saved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, input.Expected.RequestID, reportedRaw, body)
			if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
				return desktop.InputReceipt{}, err
			}
			reentry = initial
			reentry.Kind, reentry.RequestID, reentry.ReloadNonce = "ready", input.Expected.RequestID, input.Load.ReloadNonce
			reentry.RuntimeEpoch, reentry.Sequence, reentry.InputReady = initial.RuntimeEpoch+1, 1, true
			frames.signals = append(frames.signals, reentry)
		case strings.HasPrefix(command, "/dev bridge bugs-ack "):
			ack := reported
			ack.Kind, ack.Sequence = "acknowledged", reported.Sequence+1
			frames.signals = append(frames.signals, ack)
		default:
			return desktop.InputReceipt{}, fmt.Errorf("unexpected command %q", command)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 1, SubmissionComplete: true}, nil
	}
	verified, err := op.execute(ctx, send)
	if err != nil || verified.Stage != "verified" || verified.Intent.Goal != "verified" {
		t.Fatalf("bugs verification: %+v %v", verified, err)
	}
	if len(commands) != 2 {
		t.Fatalf("verification commands = %#v", commands)
	}
	if _, err := book.SetGoal(ctx, record.OperationID, "verified", "cleaned"); err != nil {
		t.Fatal(err)
	}
	completed, err := op.execute(ctx, send)
	if err != nil || completed.Stage != "cleaned" || completed.Status != "completed" {
		t.Fatalf("bugs acknowledgement: %+v %v", completed, err)
	}
	if len(commands) != 3 || !strings.Contains(commands[2], "bugs-ack") {
		t.Fatalf("ack forced another reload: %#v", commands)
	}
	stored, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || stored.Stage != "cleaned" {
		t.Fatalf("stored bugs operation: %+v %v", stored, err)
	}
}

// A separate `live ack` process resumes the faults operation with a reader that
// has observed nothing yet. The ack input path must observe fresh input-ready
// evidence itself before queueing the command; the input guard rejects an
// observation-free reader (bridge.signal_not_observed) exactly like the native
// QueuePreparedCommand ordering does.
func TestFaultsAckFreshProcessObservesReadinessBeforeInput(t *testing.T) {
	ctx := context.Background()
	root, client, book, pin, template, _ := unpreparedProbeFixture(t, 7)
	initial := template.ready
	template.Close()
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
	session, err := observeWindowSession(ctx, template.target, initialExpectation(initial), frames, func(context.Context, ClientWindow) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	record, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "bugs-fresh-ack-1", Count: 2})
	if err != nil {
		t.Fatal(err)
	}
	op, err := session.openFaultOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	input, _, _ := faultInput(record)
	body := `{"schema":"lycheedev.bugs.v1","status":"completed","complete":true,"snapshot":{"returnedCount":2}}`
	reported := bridge.Signal{Schema: "lycheedev.signal.v1", Release: initial.Release, Kind: "reported", SessionNonce: initial.SessionNonce,
		RequestID: input.Expected.RequestID, Character: initial.Character, Realm: initial.Realm, Product: initial.Product, Build: initial.Build,
		Sequence: initial.Sequence + 1, ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	reportedRaw, _ := json.Marshal(reported)
	var reentry bridge.Signal
	send := func(ctx context.Context, _ desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		switch {
		case strings.HasPrefix(command, "/dev bridge bugs "):
			frames.signals = append(frames.signals, reported)
		case strings.HasPrefix(command, "/dev bridge flush "):
			source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
			if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
				return desktop.InputReceipt{}, err
			}
			saved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, input.Expected.RequestID, reportedRaw, body)
			if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
				return desktop.InputReceipt{}, err
			}
			reentry = initial
			reentry.Kind, reentry.RequestID, reentry.ReloadNonce = "ready", input.Expected.RequestID, input.Load.ReloadNonce
			reentry.RuntimeEpoch, reentry.Sequence, reentry.InputReady = initial.RuntimeEpoch+1, 1, true
			frames.signals = append(frames.signals, reentry)
		default:
			return desktop.InputReceipt{}, fmt.Errorf("unexpected command %q", command)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 1, SubmissionComplete: true}, nil
	}
	// Process A: drive to verified. The reload receipt is archived as evidence.
	verified, err := op.execute(ctx, send)
	if err != nil || verified.Stage != "verified" {
		t.Fatalf("bugs verification: %+v %v", verified, err)
	}
	if err := op.close(); err != nil {
		t.Fatal(err)
	}
	session.Close()
	if _, err := book.SetGoal(ctx, record.OperationID, "verified", "cleaned"); err != nil {
		t.Fatal(err)
	}
	// Process B: fresh reader over the still-displayed reentry receipt.
	restart := &lifecycleFrames{t: t, signals: []bridge.Signal{reentry}}
	var acked []string
	resend := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if target != template.target.Window {
			t.Fatal("changed target")
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if !strings.HasPrefix(command, "/dev bridge bugs-ack ") {
			return desktop.InputReceipt{}, fmt.Errorf("unexpected command %q", command)
		}
		acked = append(acked, command)
		ack := reported
		ack.Kind, ack.Sequence = "acknowledged", reported.Sequence+1
		restart.signals = append(restart.signals, ack)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 1, SubmissionComplete: true}, nil
	}
	completed, err := resumeFaults(ctx, root, record.OperationID, image.Rectangle{}, func(ctx context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
		if target != template.target || region != (image.Rectangle{}) {
			t.Fatal("recovery changed target")
		}
		return restart, nil
	}, template.confirm, resend)
	if err != nil || completed.Stage != "cleaned" || completed.Status != "completed" {
		t.Fatalf("fresh-process ack: %+v %v", completed, err)
	}
	if len(acked) != 1 || !strings.Contains(acked[0], "bugs-ack") {
		t.Fatalf("ack commands: %#v", acked)
	}
	if !restart.closed {
		t.Fatal("recovered capture leaked")
	}
}

// A persisted ack intent whose recorded receipt proves zero keyboard messages
// reached the window (the pre-fix ack path recorded exactly this) must be
// re-sent on recovery, not blocked waiting for an acknowledgement that was
// never requested.
func testFaultsAckRecovery(t *testing.T, prior desktop.InputReceipt) {
	ctx := context.Background()
	root, client, book, pin, template, _ := unpreparedProbeFixture(t, 7)
	initial := template.ready
	template.Close()
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
	session, err := observeWindowSession(ctx, template.target, initialExpectation(initial), frames, func(context.Context, ClientWindow) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	record, err := session.prepareFaults(ctx, root, pin.ID, BugsRequest{Session: "saved", Account: "Account-A", Request: "bugs-unsent-ack-1", Count: 2})
	if err != nil {
		t.Fatal(err)
	}
	op, err := session.openFaultOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	input, _, _ := faultInput(record)
	body := `{"schema":"lycheedev.bugs.v1","status":"completed","complete":true,"snapshot":{"returnedCount":2}}`
	reported := bridge.Signal{Schema: "lycheedev.signal.v1", Release: initial.Release, Kind: "reported", SessionNonce: initial.SessionNonce,
		RequestID: input.Expected.RequestID, Character: initial.Character, Realm: initial.Realm, Product: initial.Product, Build: initial.Build,
		Sequence: initial.Sequence + 1, ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	reportedRaw, _ := json.Marshal(reported)
	reentry := initial
	reentry.Kind, reentry.RequestID, reentry.ReloadNonce = "ready", input.Expected.RequestID, input.Load.ReloadNonce
	reentry.RuntimeEpoch, reentry.Sequence, reentry.InputReady = initial.RuntimeEpoch+1, 1, true
	send := func(ctx context.Context, _ desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		switch {
		case strings.HasPrefix(command, "/dev bridge bugs "):
			frames.signals = append(frames.signals, reported)
		case strings.HasPrefix(command, "/dev bridge flush "):
			source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
			if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
				return desktop.InputReceipt{}, err
			}
			saved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, input.Expected.RequestID, reportedRaw, body)
			if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
				return desktop.InputReceipt{}, err
			}
			frames.signals = append(frames.signals, reentry)
		default:
			return desktop.InputReceipt{}, fmt.Errorf("unexpected command %q", command)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 1, SubmissionComplete: true}, nil
	}
	verified, err := op.execute(ctx, send)
	if err != nil || verified.Stage != "verified" {
		t.Fatalf("bugs verification: %+v %v", verified, err)
	}
	if err := op.close(); err != nil {
		t.Fatal(err)
	}
	session.Close()
	// Retain the interrupted submission evidence without inferring ACK success.
	var observed reportObservation
	if err := json.Unmarshal(verified.Observation, &observed); err != nil {
		t.Fatal(err)
	}
	observed.AckInput = &prior
	raw, _ := json.Marshal(observed)
	if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: verified.Generation, ExpectedStage: "verified", Stage: "ack_requested", Status: "unresolved", Observation: raw}); err != nil {
		t.Fatal(err)
	}
	if _, err := book.SetGoal(ctx, record.OperationID, "ack_requested", "cleaned"); err != nil {
		t.Fatal(err)
	}
	restart := &lifecycleFrames{t: t}
	var acked []string
	resend := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		restart.signals = append(restart.signals, reentry)
		if target != template.target.Window {
			t.Fatal("changed target")
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if !strings.HasPrefix(command, "/dev bridge bugs-ack ") {
			return desktop.InputReceipt{}, fmt.Errorf("unexpected command %q", command)
		}
		acked = append(acked, command)
		ack := reported
		ack.Kind, ack.Sequence = "acknowledged", reported.Sequence+1
		restart.signals = append(restart.signals, ack)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 1, SubmissionComplete: true}, nil
	}
	// The bound keeps the pre-fix unbounded acknowledgement wait from hanging
	// the suite; recovery must finish well inside it.
	bounded, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	completed, err := resumeFaults(bounded, root, record.OperationID, image.Rectangle{}, func(ctx context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
		return restart, nil
	}, template.confirm, resend)
	if err != nil || completed.Stage != "cleaned" || completed.Status != "completed" {
		t.Fatalf("unsent-intent recovery: %+v %v", completed, err)
	}
	if len(acked) != 1 {
		t.Fatalf("resent ack commands: %#v", acked)
	}
}

func TestFaultsAckRecoversUnsentPartialAndLostReceipt(t *testing.T) {
	for name, prior := range map[string]desktop.InputReceipt{"unsent": {}, "partial": {MessagesQueued: 2}, "lost-receipt": {MessagesQueued: 42, SubmissionComplete: true}} {
		t.Run(name, func(t *testing.T) { testFaultsAckRecovery(t, prior) })
	}
}
