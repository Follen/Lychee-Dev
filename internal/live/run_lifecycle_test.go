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

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestRunProbeLifecycle(t *testing.T) {
	for _, mode := range []string{"complete", "report-not-persisted", "removal-not-persisted"} {
		t.Run(mode, func(t *testing.T) {
			testRunProbeLifecycle(t, mode)
		})
	}
}

func testRunProbeLifecycle(t *testing.T, mode string) {
	t.Helper()
	ctx := context.Background()
	root, client, _, pin, original, input := unpreparedProbeFixture(t, 9)
	original.region = image.Rect(10, 20, 610, 620)
	candidate := original.ready
	connectionFrames := &lifecycleFrames{t: t, signals: []bridge.Signal{candidate}}
	connection, err := bindWindowSession(ctx, root, WindowBindingRequest{Snapshot: pin.ID, Region: original.region},
		func(_ context.Context, directory string, pid uint32) (ClientWindow, error) {
			if directory != "" || pid != 0 {
				t.Fatal("first binding required guessed window identity")
			}
			return original.target, nil
		},
		func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
			if expected.SessionNonce != "" || expected.Character != "" || expected.Realm != "" {
				t.Fatal("first binding required manual protocol identity")
			}
			session, err := observeWindowSession(ctx, target, expected, connectionFrames, func(context.Context, ClientWindow) error { return nil })
			if err == nil {
				session.region = region
			}
			return session, err
		})
	if err != nil {
		t.Fatal(err)
	}
	if !connectionFrames.closed {
		t.Fatal("binding leaked the capture stream")
	}
	bound := connection.Record
	original.Close()

	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{candidate}}
	open := func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
		if target != original.target || region != original.region || expected.SessionNonce != candidate.SessionNonce || !expected.RequireInputReady {
			t.Fatal("run changed the saved window identity")
		}
		return observeWindowSession(ctx, target, expected, frames, func(ctx context.Context, target ClientWindow) error {
			if target != original.target {
				t.Fatal("run retargeted its input window")
			}
			return nil
		})
	}

	body := `{"answer":42}`
	source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
	if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(client, "WTF", "Account", input.Load.Account, candidate.Realm, candidate.Character), 0700); err != nil {
		t.Fatal(err)
	}
	commands := make([]string, 0, 6)
	var definition bridge.ProbeDefinition

	baseSignal := func(kind string, sequence uint64) bridge.Signal {
		signal := bridge.Signal{
			Schema: "lycheedev.signal.v1", Release: definition.Release, Kind: kind,
			SessionNonce: definition.SessionNonce, RequestID: definition.RequestID,
			Character: definition.Character, Realm: definition.Realm,
			Product: definition.Product, Build: definition.Build, Sequence: sequence,
		}
		if kind == "ready" || kind == "cleared" {
			signal.GUID = definition.GUID
		}
		return signal
	}
	readySignal := func(sequence, epoch uint64) bridge.Signal {
		signal := baseSignal("ready", sequence)
		signal.RequestID, signal.ReloadNonce, signal.RuntimeEpoch, signal.InputReady = "", "", epoch, true
		return signal
	}
	bootstrapSignal := func(sequence, epoch uint64) bridge.Signal {
		signal := baseSignal("ready", sequence)
		signal.ReloadNonce, signal.RuntimeEpoch, signal.InputReady = definition.ReloadNonce, epoch, true
		return signal
	}
	loadedSignal := func() bridge.Signal {
		signal := baseSignal("loaded", 10)
		signal.ReloadNonce, signal.InputReady = definition.ReloadNonce, true
		signal.CodeBytes, signal.CodeAdler32 = uint32(len(input.Code)), fmt.Sprintf("%08x", adler32.Checksum(input.Code))
		return signal
	}
	reportedSignal := func() bridge.Signal {
		signal := baseSignal("reported", 11)
		signal.InputReady = false
		signal.CodeBytes, signal.CodeAdler32 = uint32(len(input.Code)), fmt.Sprintf("%08x", adler32.Checksum(input.Code))
		signal.ReportBytes, signal.ReportAdler32 = uint32(len(body)), fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))
		return signal
	}
	reloadedSignal := func() bridge.Signal {
		signal := bootstrapSignal(1, 3)
		return signal
	}
	acknowledgedSignal := func() bridge.Signal {
		signal := reportedSignal()
		signal.Kind, signal.Sequence = "acknowledged", 12
		return signal
	}

	sendFake := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if target != original.target.Window {
			t.Fatal("run changed the saved window identity")
		}
		step := len(commands)
		switch step {
		case 0:
			frames.signals = append(frames.signals, candidate)
		case 1:
			frames.signals = append(frames.signals, bootstrapSignal(1, 2))
		case 2:
		case 3:
			frames.signals = append(frames.signals, readySignal(12, 2))
		case 4:
			frames.signals = append(frames.signals, reloadedSignal())
		case 5:
			frames.signals = append(frames.signals, readySignal(13, 3))
		default:
			t.Fatal("run submitted more than six commands")
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		if step == 0 {
			definitions := operationQueue(t, client)
			if len(definitions) != 1 {
				t.Fatalf("run did not publish one probe definition: %+v", definitions)
			}
			definition = definitions[0]
			if definition.RequestID == input.Expected.RequestID || definition.ReloadNonce == input.Load.ReloadNonce || definition.Code != string(input.Code) {
				t.Fatalf("run did not create a fresh frozen identity: %+v", definition)
			}
		}
		current, err := runLifecycleWorkRecord(ctx, root)
		if err != nil {
			t.Fatal(err)
		}
		frozen, _, err := probeDefinition(current)
		if err != nil || frozen.Load.Account != input.Load.Account {
			t.Fatal("automatic account was not frozen", err)
		}
		if step == 0 {
			// Later directory changes cannot retarget a running operation or its
			// report read. Recovery consumes the same immutable account selection.
			if err := os.MkdirAll(filepath.Join(client, "WTF", "Account", "Late-Other", candidate.Realm, candidate.Character), 0700); err != nil {
				t.Fatal(err)
			}
		}
		wantStages := []string{"load_requested", "load_requested", "dispatch_requested", "flush_requested", "ack_requested", "acknowledged"}
		if current.Stage != wantStages[step] {
			t.Fatalf("input before durable intent at step %d: %s", step, current.Stage)
		}
		wantCommands := []string{
			"/dev bridge prepare " + definition.RequestID + " " + definition.ReloadNonce,
			"/dev bridge load " + definition.RequestID,
			"/dev bridge run " + definition.RequestID,
			"/dev bridge reload " + definition.RequestID,
			fmt.Sprintf("/dev bridge ack %s %d", definition.RequestID, reportedSignal().Sequence),
			"",
		}
		if step < 5 && command != wantCommands[step] {
			t.Fatalf("unexpected command at step %d: %s", step, command)
		}
		commands = append(commands, command)

		switch step {
		case 0:
			frames.signals = append(frames.signals, bootstrapSignal(1, 2))
		case 1:
			frames.signals = append(frames.signals, loadedSignal())
		case 2:
			frames.signals = append(frames.signals, reportedSignal())
		case 3:
			reported := reportedSignal()
			raw, err := json.Marshal(reported)
			if err != nil {
				t.Fatal(err)
			}
			saved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, definition.RequestID, raw, body)
			if mode != "report-not-persisted" {
				if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
					t.Fatal(err)
				}
			}
			frames.signals = append(frames.signals, reloadedSignal())
		case 4:
			frames.signals = append(frames.signals, acknowledgedSignal())
		case 5:
			var observation reportObservation
			if err := json.Unmarshal(current.Observation, &observation); err != nil {
				t.Fatal(err)
			}
			if observation.CleanupNonce == "" || command != "/dev bridge clean "+definition.RequestID+" "+observation.CleanupNonce {
				t.Fatalf("cleanup command did not use the durable challenge: %s", command)
			}
			if len(operationQueue(t, client)) != 0 {
				t.Fatal("cleanup sent before queue retirement")
			}
			if mode != "removal-not-persisted" {
				if err := os.WriteFile(source, []byte(`LycheeToolkitDB={schema=1,reports={}}`), 0600); err != nil {
					t.Fatal(err)
				}
			}
			cleared := baseSignal("cleared", 1)
			cleared.CleanupNonce, cleared.RuntimeEpoch, cleared.InputReady = observation.CleanupNonce, 4, false
			cleared.ReloadNonce = ""
			frames.signals = append(frames.signals, cleared)
		}
		return desktop.InputReceipt{MessagesQueued: len(command) + 1, SubmissionComplete: true}, nil
	}

	record, runErr := runProbe(ctx, root, RunRequest{Session: bound.ID, Code: input.Code}, open, sendFake)
	if !frames.closed {
		t.Fatal("run leaked the capture stream")
	}
	outcome, outcomeErr := finishOutcome(ctx, root, record, runErr)
	if record.OperationID == "" || outcome.OperationID != record.OperationID || outcome.Snapshot != pin.ID {
		t.Fatalf("run lost its recovery identity: %+v", outcome)
	}
	if len(commands) != map[string]int{"complete": 6, "report-not-persisted": 4, "removal-not-persisted": 6}[mode] {
		t.Fatalf("unexpected submitted commands: %v pending-signals=%d record=%+v run=%v finish=%v outcome=%+v", commands, len(frames.signals), record, runErr, outcomeErr, outcome)
	}

	switch mode {
	case "complete":
		if runErr != nil || outcomeErr != nil || outcome.Report.State != "verified" || !outcome.Complete || outcome.Cleanup != "complete" || outcome.Status != "completed" || string(outcome.Report.Content) != body {
			t.Fatalf("complete outcome did not retain verified report: %+v run=%v finish=%v", outcome, runErr, outcomeErr)
		}
	case "report-not-persisted":
		if runErr == nil || outcomeErr == nil || outcome.Report.State != "unavailable" || outcome.Complete || outcome.Cleanup != "pending" {
			t.Fatalf("missing report was claimed complete: %+v run=%v finish=%v", outcome, runErr, outcomeErr)
		}
	case "removal-not-persisted":
		if runErr == nil || outcomeErr == nil || outcome.Report.State != "verified" || string(outcome.Report.Content) != body || outcome.Complete || outcome.Cleanup != "pending" {
			t.Fatalf("verified report was hidden by cleanup failure: %+v run=%v finish=%v", outcome, runErr, outcomeErr)
		}
	}
}

func runLifecycleWorkRecord(ctx context.Context, root string) (journal.WorkRecord, error) {
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		documents, err := metadata.ListDocuments(ctx, "work/", "", 2)
		if err != nil {
			return journal.WorkRecord{}, err
		}
		if len(documents) != 1 {
			return journal.WorkRecord{}, fmt.Errorf("expected one live work record, got %d", len(documents))
		}
		var record journal.WorkRecord
		if err := json.Unmarshal(documents[0].Value, &record); err != nil {
			return journal.WorkRecord{}, err
		}
		if record.OperationID != strings.TrimPrefix(documents[0].Key, "work/") {
			return journal.WorkRecord{}, fmt.Errorf("work record identity mismatch: %s", documents[0].Key)
		}
		return record, nil
	})
}
