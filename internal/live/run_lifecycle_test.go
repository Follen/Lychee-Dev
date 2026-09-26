package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"hash/adler32"
	"image"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
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

func TestAtomicProbeLifecycleStopsAtEachGoal(t *testing.T) {
	testRunProbeLifecycle(t, "atomic")
}

func TestAtomicProbeLifecycleWithCompactReceipts(t *testing.T) {
	testRunProbeLifecycle(t, "atomic", true)
}

func TestAtomicProbeRecoversPersistedReportWithoutReloadReceipt(t *testing.T) {
	testRunProbeLifecycle(t, "lost-reentry")
}

func TestAtomicProbeOfflineRecovery(t *testing.T) {
	testRunProbeLifecycle(t, "offline-recovery")
}

func TestAbandonVerifiedProbeRetainsReportAndReleasesWindow(t *testing.T) {
	for _, mode := range []string{"normal", "intent-crash", "queue-crash", "conflict", "other-entry", "invalid-report"} {
		t.Run(mode, func(t *testing.T) { testRunProbeLifecycle(t, "abandon-"+mode) })
	}
}

func TestAtomicProbeAckRestartRecoversWithoutReexecutingProbe(t *testing.T) {
	for _, mode := range []string{"before-input", "after-input", "lost-receipt", "lost-permanent", "partial-input"} {
		t.Run(mode, func(t *testing.T) { testRunProbeLifecycle(t, "ack-restart-"+mode) })
	}
}

func TestAtomicProbeMissingReloadRejectsUnsafeAckReadiness(t *testing.T) {
	for _, mode := range []string{"missing", "actor", "nonce", "request", "reload-nonce", "old-epoch", "future-epoch", "unready", "payload"} {
		t.Run(mode, func(t *testing.T) { testRunProbeLifecycle(t, "unsafe-ack-"+mode) })
	}
}

func testRunProbeLifecycle(t *testing.T, mode string, compact ...bool) {
	t.Helper()
	unsafeAck := strings.HasPrefix(mode, "unsafe-ack-")
	ackRestart := strings.HasPrefix(mode, "ack-restart-")
	abandon := strings.HasPrefix(mode, "abandon-")
	atomic := mode == "atomic" || mode == "finish" || mode == "lost-reentry" || mode == "offline-recovery" || abandon || unsafeAck || ackRestart
	interrupted := errors.New("fixture: interrupted after flush submission")
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
	frames.compact = len(compact) > 0 && compact[0]
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
			if atomic {
				frames.signals = append(frames.signals, loadedSignal())
			}
		case 3:
			frames.signals = append(frames.signals, readySignal(12, 2))
		case 4:
			ready := reloadedSignal()
			switch strings.TrimPrefix(mode, "unsafe-ack-") {
			case "actor":
				ready.GUID = "Player-other"
			case "nonce":
				ready.SessionNonce = strings.Repeat("f", 32)
			case "request":
				ready.RequestID = "REQ-other"
			case "reload-nonce":
				ready.ReloadNonce = strings.Repeat("f", 32)
			case "old-epoch":
				ready.RuntimeEpoch--
			case "future-epoch":
				ready.RuntimeEpoch++
			case "unready":
				ready.InputReady = false
			case "payload":
				ready.CodeBytes, ready.CodeAdler32 = 1, "00000001"
			}
			if mode != "unsafe-ack-missing" {
				frames.signals = append(frames.signals, ready)
			}
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
		if step == 4 && mode == "ack-restart-before-input" {
			return desktop.InputReceipt{}, interrupted
		}
		commands = append(commands, command)
		if step == 4 && ackRestart && mode != "ack-restart-before-input" {
			return desktop.InputReceipt{MessagesQueued: len(command) + 1, SubmissionComplete: mode != "ack-restart-partial-input"}, interrupted
		}

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
			if mode != "lost-reentry" && mode != "offline-recovery" && !unsafeAck && !ackRestart {
				frames.signals = append(frames.signals, reloadedSignal())
			}
			if mode == "offline-recovery" {
				return desktop.InputReceipt{MessagesQueued: len(command) + 1, SubmissionComplete: mode != "ack-restart-partial-input"}, interrupted
			}
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

	var record journal.WorkRecord
	var runErr error
	if atomic {
		session, snapshot, err := observeRecordedSession(ctx, root, bound.ID, open)
		if err != nil {
			t.Fatal(err)
		}
		revision, err := PutProbe(ctx, root, "atomic-smoke", input.Code)
		if err != nil {
			t.Fatal(err)
		}
		record, err = session.PrepareProbeRevision(ctx, root, snapshot, "", "atomic-request", revision.Revision)
		if err != nil {
			t.Fatal(err)
		}
		op, err := session.OpenOperation(ctx, root, record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
		defer op.Close()
		defer session.Close()
		record, runErr = op.execute(ctx, sendFake)
		if runErr != nil || record.Stage != "loaded" || len(commands) != 2 {
			t.Fatalf("load crossed its boundary: stage=%s commands=%v err=%v", record.Stage, commands, runErr)
		}
		if _, err = setOperationGoal(ctx, root, record.OperationID, "loaded", "verified"); err != nil {
			t.Fatal(err)
		}
		record, runErr = op.execute(ctx, sendFake)
		if mode == "offline-recovery" {
			if !errors.Is(runErr, interrupted) || record.Stage != "flush_requested" || record.Status != "unresolved" {
				t.Fatalf("missing interruption: %+v %v", record, runErr)
			}
			// The public recovery path must not steal an active writer's lease.
			if _, err := Resume(ctx, root, record.OperationID); !errors.Is(err, journal.ErrBusy) {
				t.Fatalf("offline recovery bypassed writer: %v", err)
			}
			op.Close()
			session.Close()
			saved, err := os.ReadFile(source)
			if err != nil {
				t.Fatal(err)
			}
			// A wrong body cannot become verified or cause recovery input.
			if err := os.WriteFile(source, []byte(strings.ReplaceAll(string(saved), "answer", "wrongx")), 0600); err != nil {
				t.Fatal(err)
			}
			bad, err := Resume(ctx, root, record.OperationID)
			if err == nil || bad.Report.State != "unavailable" || bad.Stage != "flush_requested" {
				t.Fatalf("accepted corrupt report: %+v %v", bad, err)
			}
			if err := os.WriteFile(source, saved, 0600); err != nil {
				t.Fatal(err)
			}
			for attempt := 0; attempt < 2; attempt++ {
				outcome, err := Resume(ctx, root, record.OperationID)
				if err != nil || outcome.Stage != "verified" || outcome.Report.State != "verified" || string(outcome.Report.Content) != body || outcome.Complete || outcome.Cleanup != "pending" {
					t.Fatalf("offline recovery: %+v %v", outcome, err)
				}
			}
			if len(commands) != 4 || len(operationQueue(t, client)) != 1 {
				t.Fatal("offline recovery sent input or retired queue")
			}
			owner, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource)
			if err != nil || !occupied || owner.OperationID != record.OperationID {
				t.Fatal("offline recovery released owner", err)
			}
			// The fake process has no native window: only the later ACK may open
			// capture, with the original identity and a new reader.
			if _, err := setOperationGoal(ctx, root, record.OperationID, "verified", "cleaned"); err != nil {
				t.Fatal(err)
			}
			frames = &lifecycleFrames{t: t}
			record, runErr = resumeLiveOperation(ctx, root, record.OperationID, original.region,
				func(_ context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
					if target != original.target || region != original.region {
						t.Fatal("recovery retargeted capture")
					}
					return frames, nil
				}, func(context.Context, ClientWindow) error { return nil }, sendFake)
		} else {
			if runErr != nil || record.Stage != "verified" || len(commands) != 4 {
				t.Fatalf("run crossed its boundary: stage=%s commands=%v err=%v", record.Stage, commands, runErr)
			}
			if abandon {
				if _, err := Abandon(ctx, root, record.OperationID); !errors.Is(err, journal.ErrBusy) {
					t.Fatal("abandon did not refuse active writer", err)
				}
				op.Close()
				session.Close()
				change := func(stage, status string, observation json.RawMessage) {
					t.Helper()
					_, err := withOperation(ctx, root, record.OperationID, func(_ *vault.Store, metadata *vault.Metadata) (bool, error) {
						return true, journal.OpenBook(metadata).AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: observation})
					})
					if err != nil {
						t.Fatal(err)
					}
					record, err = InspectOperation(ctx, root, record.OperationID)
					if err != nil {
						t.Fatal(err)
					}
				}
				if mode == "abandon-invalid-report" || mode == "abandon-ack-submitted" {
					var observed reportObservation
					if err := json.Unmarshal(record.Observation, &observed); err != nil {
						t.Fatal(err)
					}
					if mode == "abandon-invalid-report" {
						observed.BodyID = observed.ReceiptID
					} else {
						observed.AckInput = &desktop.InputReceipt{}
					}
					raw, _ := json.Marshal(observed)
					change("verified", "running", raw)
					if _, err := Abandon(ctx, root, record.OperationID); err == nil {
						t.Fatal("abandon accepted invalid evidence or ACK")
					}
					if len(operationQueue(t, client)) != 1 {
						t.Fatal("refused abandon modified queue")
					}
					if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || !occupied {
						t.Fatal("refused abandon lost owner", err)
					}
					return
				}
				if mode == "abandon-intent-crash" || mode == "abandon-queue-crash" {
					change("abandoning", "running", record.Observation)
					if mode == "abandon-queue-crash" {
						if _, err := delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(client), definition, true); err != nil {
							t.Fatal(err)
						}
					}
					if result, err := Resume(ctx, root, record.OperationID); err != nil || result.Status != "abandoned" {
						t.Fatal("abandon recovery failed", result, err)
					}
				}
				expectedEntries := 0
				if mode == "abandon-conflict" || mode == "abandon-other-entry" {
					other := definition
					if mode == "abandon-conflict" {
						other.Character = "DifferentActor"
						if _, err := delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(client), definition, true); err != nil {
							t.Fatal(err)
						}
					} else {
						other.RequestID = "REQ-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
						expectedEntries = 1
					}
					if _, err := delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(client), other, false); err != nil {
						t.Fatal(err)
					}
					if mode == "abandon-conflict" {
						if _, err := Abandon(ctx, root, record.OperationID); !errors.Is(err, delivery.ErrQueueConflict) {
							t.Fatal("abandon ignored queue conflict", err)
						}
						if queue := operationQueue(t, client); len(queue) != 1 || queue[0] != other {
							t.Fatal("abandon touched conflicting queue")
						}
						if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || !occupied {
							t.Fatal("conflict lost owner", err)
						}
						return
					}
				}
				saved, err := os.ReadFile(source)
				if err != nil {
					t.Fatal(err)
				}
				for attempt := 0; attempt < 2; attempt++ {
					outcome, err := Abandon(ctx, root, record.OperationID)
					if err != nil || outcome.Status != "abandoned" || outcome.Cleanup != "abandoned" || outcome.Complete || outcome.Report.State != "verified" || string(outcome.Report.Content) != body {
						t.Fatalf("abandon: %+v %v", outcome, err)
					}
				}
				if len(operationQueue(t, client)) != expectedEntries || len(commands) != 4 {
					t.Fatal("abandon retained queue or sent input")
				}
				if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || occupied {
					t.Fatal("abandon retained owner", err)
				}
				after, err := os.ReadFile(source)
				if err != nil || string(after) != string(saved) {
					t.Fatal("abandon modified game saved variables", err)
				}
				resumed, err := Resume(ctx, root, record.OperationID)
				if err != nil || resumed.Status != "abandoned" {
					t.Fatal("abandoned operation resumed execution", err)
				}
				if _, err := RunLoaded(ctx, root, record.OperationID); err == nil {
					t.Fatal("abandoned operation allowed run")
				}
				if _, err := AcknowledgeVerified(ctx, root, record.OperationID); err == nil {
					t.Fatal("abandoned operation allowed ack")
				}
				return
			}
			var retained probeLoadObservation
			if err := json.Unmarshal(record.Observation, &retained); err != nil {
				t.Fatal(err)
			}
			if retained.LoadedCapture == "" || retained.LoadReadyCapture == "" || retained.ReportedCapture == "" || retained.FlushReadyCapture == "" ||
				retained.LoadInput == nil || retained.DispatchInput == nil || retained.FlushInput == nil || !retained.FlushInput.SubmissionComplete {
				t.Fatal("verified report discarded transport evidence", string(record.Observation))
			}
			if _, err = setOperationGoal(ctx, root, record.OperationID, "verified", "cleaned"); err != nil {
				t.Fatal(err)
			}
			record, runErr = op.execute(ctx, sendFake)
			op.Close()
			session.Close()
			if ackRestart {
				if !errors.Is(runErr, interrupted) || record.Stage != "ack_requested" {
					t.Fatalf("missing ACK interruption: %+v %v", record, runErr)
				}
				frames = &lifecycleFrames{t: t}
				if mode == "ack-restart-after-input" {
					frames.signals = append(frames.signals, acknowledgedSignal())
				}
				record, runErr = resumeLiveOperation(ctx, root, record.OperationID, original.region,
					func(_ context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
						if target != original.target || region != original.region {
							t.Fatal("ACK restart retargeted capture")
						}
						return frames, nil
					}, func(context.Context, ClientWindow) error { return nil },
					func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
						if target != original.target.Window {
							t.Fatal("ACK retry retargeted window")
						}
						if mode == "ack-restart-after-input" {
							t.Fatal("visible ACK was unnecessarily resent")
						}
						if mode != "ack-restart-lost-permanent" {
							frames.signals = append(frames.signals, reloadedSignal())
						}
						if err := guard(ctx); err != nil {
							return desktop.InputReceipt{}, err
						}
						command, err := prepare(ctx)
						if err != nil {
							return desktop.InputReceipt{}, err
						}
						if command != fmt.Sprintf("/dev bridge ack %s %d", definition.RequestID, reportedSignal().Sequence) {
							t.Fatal("retry changed operation or executed probe", command)
						}
						if err := guard(ctx); err != nil {
							return desktop.InputReceipt{}, err
						}
						commands = append(commands, command)
						frames.signals = append(frames.signals, acknowledgedSignal())
						return desktop.InputReceipt{MessagesQueued: len(command) + 1, SubmissionComplete: true}, nil
					})
			}
		}
	} else {
		record, runErr = executePrepared(ctx, root, ExecutionRequest{Session: bound.ID, Code: input.Code}, open, sendFake)
	}
	if !frames.closed {
		t.Fatal("run leaked the capture stream")
	}
	outcome, outcomeErr := finishOutcome(ctx, root, record, runErr)
	if record.OperationID == "" || outcome.OperationID != record.OperationID || outcome.Snapshot != pin.ID {
		t.Fatalf("run lost its recovery identity: %+v", outcome)
	}
	if ackRestart {
		if mode == "ack-restart-lost-permanent" {
			if runErr == nil || outcome.Report.State != "verified" || outcome.Cleanup != "pending" {
				t.Fatalf("lost ACK claimed success: %+v %v", outcome, runErr)
			}
			for _, recover := range []func(context.Context, string, string) (Outcome, error){Abandon, Resume, Abandon} {
				abandoned, err := recover(ctx, root, record.OperationID)
				if err != nil || abandoned.Report.State != "verified" || abandoned.Cleanup != "abandoned" || abandoned.Complete {
					t.Fatalf("lost ACK recovery: %+v %v", abandoned, err)
				}
			}
			if _, occupied, err := journal.InspectWindowOwner(ctx, filepath.Join(client, "Interface", "AddOns"), record.Intent.Resource); err != nil || occupied {
				t.Fatal("abandon retained owner", err)
			}
		} else {
			wantCommands := 6
			if mode == "ack-restart-before-input" || mode == "ack-restart-after-input" {
				wantCommands = 5
			}
			if runErr != nil || outcomeErr != nil || len(commands) != wantCommands || !outcome.Complete || outcome.Report.State != "verified" || outcome.Cleanup != "complete" || len(operationQueue(t, client)) != 0 {
				t.Fatalf("ACK recovery failed: commands=%v %+v %v", commands, outcome, runErr)
			}
		}
		return
	}
	if unsafeAck {
		if mode == "unsafe-ack-missing" && !errors.Is(runErr, ErrAckReadinessPending) {
			t.Fatalf("missing readiness is not recoverable pending: %v", runErr)
		}
		if runErr == nil || outcomeErr == nil || len(commands) != 4 || record.Stage != "verified" || outcome.Report.State != "verified" || outcome.Complete || outcome.Cleanup != "pending" {
			t.Fatalf("unsafe ACK: commands=%v outcome=%+v err=%v", commands, outcome, runErr)
		}
		return
	}
	if len(commands) != map[string]int{"offline-recovery": 5, "atomic": 5, "finish": 5, "lost-reentry": 5, "complete": 6, "report-not-persisted": 4, "removal-not-persisted": 6}[mode] {
		t.Fatalf("unexpected submitted commands: %v pending-signals=%d record=%+v run=%v finish=%v outcome=%+v", commands, len(frames.signals), record, runErr, outcomeErr, outcome)
	}

	switch mode {
	case "complete", "atomic", "finish", "lost-reentry", "offline-recovery":
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
	if mode == "finish" {
		savedInput, inputErr := reportInput(record)
		if inputErr != nil {
			t.Fatal(inputErr)
		}
		for attempt := 0; attempt < 3; attempt++ {
			finished, err := finishProbe(ctx, root, record.OperationID, AcknowledgeVerified, func(_ context.Context, gotRoot, binding string) (HideReceiptResult, error) {
				if attempt == 2 {
					t.Fatal("completed finish called hide again")
				}
				if gotRoot != root || binding != savedInput.Binding {
					t.Fatal("finish changed session")
				}
				if attempt == 0 {
					return HideReceiptResult{}, ErrReceiptHidePending
				}
				return HideReceiptResult{Cleared: true, Session: binding, ObservedAt: time.Now()}, nil
			})
			if finished.OperationID != record.OperationID || finished.Report.State != "verified" || string(finished.Report.Content) != body || finished.Cleanup != "complete" || finished.Complete != (attempt >= 1) {
				t.Fatalf("finish changed verified result: %+v", finished)
			}
			if attempt == 0 && !errors.Is(err, ErrReceiptHidePending) || attempt >= 1 && err != nil {
				t.Fatal(err)
			}
			if len(commands) != 5 {
				t.Fatal("finish replayed ACK", commands)
			}
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

func TestAtomicProbeFinishRetriesDisplayWithoutAckReplay(t *testing.T) {
	testRunProbeLifecycle(t, "finish")
}
