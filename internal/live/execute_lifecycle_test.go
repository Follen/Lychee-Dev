package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"hash/adler32"
	"image"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type lifecycleFrames struct {
	t       *testing.T
	signals []bridge.Signal
	ticks   int64
	closed  bool
	compact bool
}

func (f *lifecycleFrames) Close() { f.closed = true }
func (f *lifecycleFrames) Next(ctx context.Context) (*desktop.CapturedFrame, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if f.closed || len(f.signals) == 0 {
		return nil, io.EOF
	}
	signal := f.signals[0]
	if f.compact && signal.Kind != "ready" && signal.Kind != "cleared" {
		signal.Character, signal.Realm, signal.GUID = "", "", ""
		if signal.Kind == "reported" || signal.Kind == "acknowledged" || signal.Kind == "cancelled" {
			signal.SessionNonce = ""
		}
	}
	f.signals = f.signals[1:]
	f.ticks++
	frame := makeAckFrame(f.t, signal)
	frame.SystemTicks = f.ticks
	return frame, nil
}

func TestExecuteCompletesLifecycle(t *testing.T) {
	modes := []string{"complete", "report-not-persisted", "removal-not-persisted"}
	for _, phase := range []string{"bootstrap", "load", "dispatch", "flush", "ack", "clean"} {
		for _, variant := range []string{"", "missing-", "foreign-", "unrecorded-", "unsent-"} {
			modes = append(modes, "resume-"+variant+phase)
		}
	}
	for _, mode := range modes {
		t.Run(mode, func(t *testing.T) {
			// Each scenario owns its client, workspace, frame queue and journal.
			// Overlap independent filesystem work without weakening durability.
			t.Parallel()
			testExecuteLifecycle(t, mode)
		})
	}
}

func testExecuteLifecycle(t *testing.T, mode string) {
	t.Helper()
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixtureAtSequence(t, 99)
	template, _ := operationSessionFixtureAtSequence(t, client, input, 99)
	initial := template.ready
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
	expected := input.Expected
	expected.Kind, expected.RequestID, expected.AfterSequence, expected.RequireInputReady = "ready", "", 0, true
	session, err := observeWindowSession(ctx, template.target, expected, frames, template.confirm)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	body := `{"answer":42}`
	reported := initial
	reported.Kind, reported.RequestID, reported.Sequence = "reported", input.Expected.RequestID, 3
	reported.GUID, reported.RuntimeEpoch, reported.InputReady = "", 0, false
	reported.CodeBytes, reported.CodeAdler32 = uint32(len(input.Code)), fmt.Sprintf("%08x", adler32.Checksum(input.Code))
	reported.ReportBytes, reported.ReportAdler32 = uint32(len(body)), fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))
	loaded := reported
	loaded.Kind, loaded.Sequence, loaded.ReloadNonce, loaded.InputReady = "loaded", 2, input.Load.ReloadNonce, true
	loaded.ReportBytes, loaded.ReportAdler32 = 0, ""
	bootstrap := initial
	bootstrap.RequestID, bootstrap.ReloadNonce, bootstrap.RuntimeEpoch, bootstrap.Sequence = input.Expected.RequestID, input.Load.ReloadNonce, 2, 1
	reloaded := bootstrap
	reloaded.RuntimeEpoch = 3
	flushReady := initial
	flushReady.RuntimeEpoch, flushReady.Sequence = 2, 4
	ack := reported
	ack.Kind, ack.Sequence = "acknowledged", 4
	cleanupReady := initial
	cleanupReady.RuntimeEpoch, cleanupReady.Sequence = 3, 5
	source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
	if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
		t.Fatal(err)
	}
	commands := []string{}
	resumeAt := -1
	resumePhase := mode[strings.LastIndex(mode, "-")+1:]
	for index, name := range []string{"bootstrap", "load", "dispatch", "flush", "ack", "clean"} {
		if strings.HasPrefix(mode, "resume-") && resumePhase == name {
			resumeAt = index
		}
	}
	interrupted := false
	partial := errors.New("fixture.process_interrupted_after_input")
	crashed := errors.New("fixture.process_exited_before_receipt")
	send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if target != session.target.Window {
			t.Fatal("changed target")
		}
		step := len(commands)
		switch step {
		case 0:
			frames.signals = append(frames.signals, initial)
		case 1:
			frames.signals = append(frames.signals, bootstrap)
		case 2: // The loaded receipt was emitted by the previous command.
		case 3:
			frames.signals = append(frames.signals, flushReady)
		case 4:
			frames.signals = append(frames.signals, reloaded)
		case 5:
			frames.signals = append(frames.signals, cleanupReady)
		default:
			t.Fatal("unexpected extra submission")
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
		current, err := book.InspectWork(ctx, record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
		wantStages := []string{"load_requested", "load_requested", "dispatch_requested", "flush_requested", "ack_requested", "acknowledged"}
		if current.Stage != wantStages[step] {
			t.Fatal("input before intent", step, current.Stage)
		}
		want := []string{"prepare " + input.Expected.RequestID + " " + input.Load.ReloadNonce, "load " + input.Expected.RequestID, "run " + input.Expected.RequestID, "reload " + input.Expected.RequestID, fmt.Sprintf("ack %s %d", input.Expected.RequestID, reported.Sequence)}
		if step < 5 && command != "/dev bridge "+want[step] {
			t.Fatal(command)
		}
		commands = append(commands, command)
		if step == resumeAt && strings.HasPrefix(mode, "resume-unsent-") {
			// Exit after durable intent but before the first keyboard message.
			// Recovery cannot distinguish this from an input with no reply.
			frames.signals = nil
			panic(crashed)
		}
		switch step {
		case 0:
			frames.signals = append(frames.signals, bootstrap)
		case 1:
			frames.signals = append(frames.signals, loaded)
		case 2:
			frames.signals = append(frames.signals, reported)
		case 3:
			raw, _ := json.Marshal(reported)
			saved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, input.Expected.RequestID, raw, body)
			if mode != "report-not-persisted" {
				if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
					t.Fatal(err)
				}
			}
			frames.signals = append(frames.signals, reloaded)
		case 4:
			frames.signals = append(frames.signals, ack)
		case 5:
			var observation reportObservation
			if err := json.Unmarshal(current.Observation, &observation); err != nil {
				t.Fatal(err)
			}
			if command != "/dev bridge clean "+input.Expected.RequestID+" "+observation.CleanupNonce || observation.CleanupNonce == "" {
				t.Fatal(command)
			}
			if len(operationQueue(t, client)) != 0 {
				t.Fatal("cleanup before queue retirement")
			}
			if mode != "removal-not-persisted" {
				if err := os.WriteFile(source, []byte(`LycheeToolkitDB={schema=1,reports={}}`), 0600); err != nil {
					t.Fatal(err)
				}
			}
			cleared := initial
			cleared.Kind, cleared.RequestID, cleared.CleanupNonce = "cleared", input.Expected.RequestID, observation.CleanupNonce
			cleared.RuntimeEpoch, cleared.Sequence, cleared.InputReady = 4, 1, false
			frames.signals = append(frames.signals, cleared)
		}
		if step == resumeAt && !interrupted {
			interrupted = true
			if strings.HasPrefix(mode, "resume-unrecorded-") {
				// The game took the input, but the host never saved its input
				// receipt. Only the pre-send intent/evidence survives.
				panic(crashed)
			}
			return desktop.InputReceipt{MessagesQueued: len(command) + 3}, partial
		}
		return desktop.InputReceipt{MessagesQueued: len(command) + 4, SubmissionComplete: true}, nil
	}
	completed, err := func() (record journal.WorkRecord, err error) {
		defer func() {
			if fault := recover(); fault != nil {
				if fault != crashed {
					panic(fault)
				}
				var readErr error
				record, readErr = book.InspectWork(ctx, operation.id)
				err = errors.Join(crashed, readErr)
			}
		}()
		return operation.execute(ctx, send)
	}()
	if resumeAt >= 0 {
		crashMode := strings.HasPrefix(mode, "resume-unrecorded-") || strings.HasPrefix(mode, "resume-unsent-")
		if crashMode {
			if !errors.Is(err, crashed) || completed.Status != "running" {
				t.Fatal("missing pre-receipt crash outcome", completed, err)
			}
			var observation map[string]json.RawMessage
			if err := json.Unmarshal(completed.Observation, &observation); err != nil {
				t.Fatal(err)
			}
			key := []string{"bootstrapInput", "loadInput", "dispatchInput", "flushInput", "ackInput", "cleanupInput"}[resumeAt]
			if _, exists := observation[key]; exists {
				t.Fatal("fixture saved an input receipt after process exit", key)
			}
		} else if !errors.Is(err, partial) || completed.Status != "unresolved" {
			t.Fatal("missing interrupted outcome", err)
		}
		interruptedRecord := completed
		if strings.HasPrefix(mode, "resume-missing-") {
			frames.signals = nil
		}
		if strings.HasPrefix(mode, "resume-foreign-") {
			for i := range frames.signals {
				frames.signals[i].SessionNonce = strings.Repeat("f", 32)
			}
		}
		if err := operation.Close(); err != nil {
			t.Fatal(err)
		}
		session.Close()
		// New host lifetime: no live session or OS lease survives. Retain only
		// the fixture's game display and persisted operation/evidence/files.
		frames = &lifecycleFrames{t: t, signals: append([]bridge.Signal(nil), frames.signals...)}
		completed, err = resumeLiveOperation(ctx, root, record.OperationID, image.Rectangle{}, func(ctx context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
			if target != template.target || region != (image.Rectangle{}) {
				t.Fatal("recovery changed target")
			}
			return frames, nil
		}, template.confirm, send)
		if !frames.closed {
			t.Fatal("recovered capture leaked")
		}
		if strings.HasPrefix(mode, "resume-missing-") || strings.HasPrefix(mode, "resume-foreign-") || strings.HasPrefix(mode, "resume-unsent-") {
			if err == nil || completed.OperationID != record.OperationID || completed.Stage != interruptedRecord.Stage || completed.Generation != interruptedRecord.Generation || len(commands) != resumeAt+1 {
				t.Fatalf("missing/foreign evidence advanced or replayed: %+v %v commands=%v", completed, err, commands)
			}
			return
		}
	}
	if mode == "report-not-persisted" || mode == "removal-not-persisted" {
		wantStage, wantCommands := "flush_requested", 4
		if mode == "removal-not-persisted" {
			wantStage, wantCommands = "acknowledged", 6
		}
		if err == nil || completed.OperationID != record.OperationID || completed.Stage != wantStage || completed.Status == "completed" || len(commands) != wantCommands {
			t.Fatalf("disk failure claimed success: %+v %v commands=%v", completed, err, commands)
		}
		if _, err := ReleaseCompletedProbe(ctx, root, completed.OperationID); err == nil {
			t.Fatal("released incomplete work")
		}
		result, resultErr := Status(ctx, root, completed.OperationID)
		if resultErr != nil || result.Complete || result.Cleanup != "pending" {
			t.Fatal("read-only result claimed cleanup", result, resultErr)
		}
		if mode == "removal-not-persisted" {
			if result.Report.State != "verified" || string(result.Report.Content) != body {
				t.Fatal("cleanup failure hid valid report", result)
			}
			var corrupted reportObservation
			if err := json.Unmarshal(completed.Observation, &corrupted); err != nil {
				t.Fatal(err)
			}
			corrupted.BodyID = corrupted.ReceiptID
			raw, _ := json.Marshal(corrupted)
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: completed.OperationID, ExpectedGeneration: completed.Generation, ExpectedStage: completed.Stage, Stage: completed.Stage, Status: completed.Status, Observation: raw}); err != nil {
				t.Fatal(err)
			}
			invalid, err := Status(ctx, root, completed.OperationID)
			if err == nil || invalid.Report.State == "verified" || len(invalid.Report.Content) != 0 || invalid.Complete {
				t.Fatal("status trusted a mismatched archive", invalid, err)
			}
		} else if result.Report.State != "unavailable" || len(result.Report.Content) != 0 {
			t.Fatal("unpersisted report claimed verified", result)
		}
		return
	}
	if err != nil {
		t.Fatalf("execute at %s after %s: %v", completed.Stage, strings.Join(commands, "; "), err)
	}
	if completed.Stage != "cleaned" || completed.Status != "completed" || len(commands) != 6 || len(frames.signals) != 0 {
		t.Fatalf("incomplete: %+v commands=%v", completed, commands)
	}
	var proof reportObservation
	if err := json.Unmarshal(completed.Observation, &proof); err != nil || proof.BodyID == "" || proof.ReceiptID == "" || proof.ClearedID == "" || proof.RemovalID == "" {
		t.Fatal("completion without archive chain", err)
	}
	if _, err := ReleaseCompletedProbe(ctx, root, completed.OperationID); err != nil {
		t.Fatal("terminal retirement not idempotent", err)
	}
	result, err := Status(ctx, root, completed.OperationID)
	if err != nil || !result.Complete || result.Report.State != "verified" || string(result.Report.Content) != body || result.Cleanup != "complete" {
		t.Fatal("completed result not readable", result, err)
	}
}
