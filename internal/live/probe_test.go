package live

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
	"hash/adler32"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func probeOperationFixture(t *testing.T) (string, string, *journal.Book, journal.WorkRecord, ReportIntent) {
	return probeOperationFixtureAtSequence(t, 1)
}

func probeOperationFixtureAtSequence(t *testing.T, sequence uint64) (string, string, *journal.Book, journal.WorkRecord, ReportIntent) {
	root, client, book, pin, session, input := unpreparedProbeFixture(t, sequence)
	record, err := session.PrepareProbe(context.Background(), root, pin.ID, input.Load.Account, input.Code)
	if err != nil {
		t.Fatal(err)
	}
	input, _, err = probeDefinition(record)
	if err != nil {
		t.Fatal(err)
	}
	return root, client, book, record, input
}

func unpreparedProbeFixture(t *testing.T, sequence uint64) (string, string, *journal.Book, selection.PinnedSet, *WindowSession, ReportIntent) {
	t.Helper()
	ctx := context.Background()
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, testkit.Release(t, "queue"), client, testkit.Version); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { metadata.Close() })
	input := ReportIntent{Schema: "lycheedev.report-intent.v1", Expected: bridge.SignalExpectation{Kind: "reported", Release: testkit.Version, SessionNonce: strings.Repeat("a", 32), RequestID: "OP-probe", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.69875", AfterSequence: 1}, Code: []byte("return {answer=42}"), Load: &ProbeLoadIntent{Installation: client, GUID: "Player-1-123", ReloadNonce: strings.Repeat("b", 32)}}
	input.Load.Account = "Account-A"
	book := journal.OpenBook(metadata)
	pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "retail", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}})
	if err != nil {
		t.Fatal(err)
	}
	session, _ := operationSessionFixtureAtSequence(t, client, input, sequence)
	return root, client, book, pin, session, input
}
func operationQueue(t *testing.T, client string) []bridge.ProbeDefinition {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(client, "Interface", "AddOns", "Lychee Dev", "Bridge", "Definitions.lua"))
	if err != nil {
		t.Fatal(err)
	}
	definitions, err := bridge.DecodeProbeQueue(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	return definitions
}

func TestOperationReportLifecycleUsesArchivedEvidence(t *testing.T) {
	t.Run("bootstrap-full-lifecycle", func(t *testing.T) { testOperationFilePreparation(t, false, "cleanup-success-bootstrap") })
	t.Run("resume-persisted", func(t *testing.T) { testOperationFilePreparation(t, true, "success") })
	t.Run("from-flush-requested", func(t *testing.T) { testOperationFilePreparation(t, false, "success") })
	t.Run("retire-before-flush", func(t *testing.T) { testOperationFilePreparation(t, false, "retire-before-flush") })
	for _, mode := range []string{"success", "partial", "cancelled", "unready", "pre-ack"} {
		t.Run("cleanup-"+mode, func(t *testing.T) { testOperationFilePreparation(t, false, "cleanup-"+mode) })
	}
	for _, mode := range []string{"partial", "cancelled"} {
		t.Run("ack-"+mode, func(t *testing.T) { testOperationFilePreparation(t, false, mode) })
	}
}

func testOperationFilePreparation(t *testing.T, persisted bool, ackMode string) {
	t.Helper()
	ctx := context.Background()
	withBootstrap := strings.HasSuffix(ackMode, "-bootstrap")
	ackMode = strings.TrimSuffix(ackMode, "-bootstrap")
	sequence := uint64(1)
	if withBootstrap {
		sequence = 99
	}
	root, client, book, record, input := probeOperationFixtureAtSequence(t, sequence)
	if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
		t.Fatal(err)
	}
	e := input.Expected
	body := `{"answer":42}`
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: e.Release, Kind: "reported", SessionNonce: e.SessionNonce, RequestID: e.RequestID, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 10, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code)), ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum([]byte(body)))}
	loaded := signal
	loaded.Kind, loaded.Sequence, loaded.ReloadNonce, loaded.InputReady = "loaded", 2, input.Load.ReloadNonce, true
	loaded.ReportBytes, loaded.ReportAdler32 = 0, ""
	session, frames := operationSessionFixtureAtSequence(t, client, input, sequence)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	var tickOffset int64
	var bootstrapID string
	if withBootstrap {
		bootstrapID = exerciseBootstrap(t, operation, session, frames, book, record, input)
		tickOffset = 1000
	}
	frames.frame = makeAckFrame(t, loaded)
	frames.frame.SystemTicks = 2 + tickOffset
	if _, err := operation.Observe(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := RequestOperationDispatch(ctx, root, record.OperationID); err != nil {
		t.Fatal(err)
	}
	frames.frame = makeAckFrame(t, signal)
	frames.frame.SystemTicks = 2 + tickOffset
	if capture, err := operation.Observe(ctx); err == nil || capture.ID != "" {
		t.Fatal("frame watermark reset between loaded and reported")
	}
	frames.frame = makeAckFrame(t, signal)
	frames.frame.SystemTicks = 3 + tickOffset
	if _, err := operation.Observe(ctx); err != nil {
		t.Fatal(err)
	}
	if command, err := RequestOperationFlush(ctx, root, record.OperationID); err != nil || command != "/dev bridge reload "+e.RequestID {
		t.Fatalf("flush: %q %v", command, err)
	}
	if command, err := RequestOperationFlush(ctx, root, record.OperationID); err == nil || command != "" {
		t.Fatal("flush replayed")
	}
	beforeReload := session.ready
	if _, err := operation.PrepareFiles(ctx); err == nil || err.Error() != "live.reload_not_observed" {
		t.Fatal("file preparation bypassed live reload confirmation", err)
	}
	reentry := beforeReload
	reentry.RequestID, reentry.ReloadNonce, reentry.RuntimeEpoch = e.RequestID, input.Load.ReloadNonce, beforeReload.RuntimeEpoch+1
	for index, mode := range []string{"old-runtime", "future-runtime", "nonce", "request", "guid", "unready", "payload"} {
		wrong := reentry
		switch mode {
		case "old-runtime":
			wrong.RuntimeEpoch--
		case "future-runtime":
			wrong.RuntimeEpoch++
		case "nonce":
			wrong.ReloadNonce = strings.Repeat("c", 32)
		case "request":
			wrong.RequestID = "other"
		case "guid":
			wrong.GUID = "Player-1-other"
		case "unready":
			wrong.InputReady = false
		case "payload":
			wrong.CodeBytes, wrong.CodeAdler32 = 1, "00000001"
		}
		frames.frame = makeAckFrame(t, wrong)
		frames.frame.SystemTicks = int64(4+index) + tickOffset
		if ref, err := operation.ObserveReload(ctx); err == nil || ref.ID != "" || session.ready != beforeReload {
			t.Fatalf("invalid reload %s accepted: %+v %v", mode, ref, err)
		}
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 20 + tickOffset
	reloadCapture, err := operation.ObserveReload(ctx)
	if err != nil || reloadCapture.ID == "" || session.ready != reentry {
		t.Fatalf("reload observation: %+v %v", reloadCapture, err)
	}
	if err := operation.check(ctx); err != nil {
		t.Fatal(err)
	}
	committedReload, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	// Model interruption after the durable capture transition but before the
	// live handle adopted the new runtime. Reconciliation still needs new pixels.
	session.ready = beforeReload
	if err := operation.check(ctx); err == nil {
		t.Fatal("stale runtime remained eligible")
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 21 + tickOffset
	reused, err := operation.ObserveReload(ctx)
	if err != nil || reused.ID != reloadCapture.ID || session.ready != reentry {
		t.Fatalf("reload reconciliation: %+v %v", reused, err)
	}
	reconciled, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || reconciled.Generation != committedReload.Generation {
		t.Fatal("reload reconciliation rewrote evidence", err)
	}
	receipt, _ := json.Marshal(signal)
	saved := fmt.Sprintf(`LycheeToolkitDB = {schema=1,reports={[%q]={receipt=%q,body=%q}}}`, e.RequestID, receipt, body)
	if _, err := ObserveInstalledOperationPersisted(ctx, root, record.OperationID); err == nil {
		t.Fatal("missing selected account source accepted")
	}
	source := filepath.Join(client, "WTF", "Account", input.Load.Account, "SavedVariables", "Lychee Dev.lua")
	if err := os.MkdirAll(filepath.Dir(source), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
		t.Fatal(err)
	}
	// Even a valid report for the same request is not interchangeable with the
	// actual observed receipt. A newer sequence must not silently replace it.
	changed := signal
	changed.Sequence++
	changedRaw, _ := json.Marshal(changed)
	wrongSaved := fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`, e.RequestID, changedRaw, body)
	if err := os.WriteFile(source, []byte(wrongSaved), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := ObserveInstalledOperationPersisted(ctx, root, record.OperationID); err == nil {
		t.Fatal("different persisted receipt accepted")
	}
	if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
		t.Fatal(err)
	}
	if persisted {
		if _, err := ObserveInstalledOperationPersisted(ctx, root, record.OperationID); err != nil {
			t.Fatal(err)
		}
		if _, err := ArchiveOperationReport(ctx, root, record.OperationID, strings.NewReader(saved)); err == nil {
			t.Fatal("arbitrary reader bypassed frozen source")
		}
		if err := os.WriteFile(source, []byte(wrongSaved), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := ArchiveInstalledOperationReport(ctx, root, record.OperationID); err == nil {
			t.Fatal("changed report archived after persistence")
		}
		if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if prepared, err := operation.PrepareFiles(ctx); err != nil || prepared.Stage != "verified" {
		t.Fatal(err)
	}
	archived, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	var provenance reportObservation
	if err := json.Unmarshal(archived.Observation, &provenance); err != nil {
		t.Fatal(err)
	}
	if provenance.SourcePath != testkit.CanonicalPath(t, source) || len(provenance.SourceSHA256) != 64 {
		t.Fatal("missing source provenance")
	}
	for i := 0; i < 2; i++ {
		prepared, err := operation.PrepareFiles(ctx)
		if err != nil || prepared.Stage != "verified" {
			t.Fatalf("prepare %d: %+v %v", i, prepared, err)
		}
	}
	definitions := operationQueue(t, client)
	_, frozenDefinition, err := probeDefinition(record)
	if err != nil || len(definitions) != 1 || definitions[0] != frozenDefinition {
		t.Fatal("report preparation changed the probe queue", err)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Stage != "verified" {
		t.Fatalf("file effect claimed game acknowledgement: %+v %v", current, err)
	}
	var archivedObservation reportObservation
	if err := json.Unmarshal(current.Observation, &archivedObservation); err != nil || archivedObservation.ReloadedCapture != reloadCapture.ID {
		t.Fatal("report archival lost runtime evidence", err)
	}
	if archivedObservation.BootstrapCapture != bootstrapID {
		t.Fatal("bootstrap proof lost during report archival")
	}
	// A changed on-disk client identity cannot be bypassed by a previously
	// prepared queue or a valid archived report.
	if err := os.WriteFile(filepath.Join(client, "version.txt"), []byte("12.1.0.99999"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := operation.prepareAcknowledgement(ctx); err == nil {
		t.Fatal("changed client accepted")
	}
	if err := os.WriteFile(filepath.Join(client, "version.txt"), []byte("12.1.0.69875"), 0600); err != nil {
		t.Fatal(err)
	}
	acknowledged := signal
	for index, mutate := range []func(*bridge.Signal){
		func(s *bridge.Signal) { s.InputReady = false },
		func(s *bridge.Signal) { s.RuntimeEpoch-- },
		func(s *bridge.Signal) { s.GUID = "Player-1-foreign" },
		func(s *bridge.Signal) { s.RequestID = "OP-foreign" },
		func(s *bridge.Signal) { s.ReloadNonce = strings.Repeat("f", 32) },
	} {
		wrong := reentry
		mutate(&wrong)
		frames.frame = makeAckFrame(t, wrong)
		frames.frame.SystemTicks = int64(22+index) + tickOffset
		if err := operation.prepareAcknowledgement(ctx); err == nil {
			t.Fatalf("invalid ACK readiness %d accepted", index)
		}
		unchanged, err := book.InspectWork(ctx, record.OperationID)
		if err != nil || unchanged.Generation != current.Generation || unchanged.Stage != "verified" {
			t.Fatal("invalid readiness changed ACK intent", err)
		}
	}
	frames.frame = makeAckFrame(t, reentry)
	frames.frame.SystemTicks = 30 + tickOffset
	ackSubmissions := 0
	ackContext, cancelAck := context.WithCancel(ctx)
	defer cancelAck()
	partialFailure := errors.New("fixture.partial_ack")
	sendAck := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		ackSubmissions++
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		if command != fmt.Sprintf("/dev bridge ack %s %d", e.RequestID, signal.Sequence) {
			t.Fatal("wrong acknowledgement", command)
		}
		intent, err := book.InspectWork(ctx, record.OperationID)
		if err != nil || intent.Stage != "ack_requested" {
			t.Fatal("ACK preceded durable intent", err)
		}
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		if ackMode == "cancelled" {
			cancelAck()
			return desktop.InputReceipt{MessagesQueued: 2}, ctx.Err()
		}
		if ackMode == "partial" {
			return desktop.InputReceipt{MessagesQueued: 3}, partialFailure
		}
		return desktop.InputReceipt{MessagesQueued: 2, SubmissionComplete: true}, nil
	}
	ackInput, ackErr := operation.acknowledge(ackContext, sendAck)
	if (ackErr == nil) != (ackMode != "partial" && ackMode != "cancelled") {
		t.Fatal("unexpected ACK outcome", ackErr)
	}
	afterInput, err := book.InspectWork(ctx, record.OperationID)
	var afterObservation reportObservation
	if err != nil || json.Unmarshal(afterInput.Observation, &afterObservation) != nil || afterObservation.AckInput == nil || *afterObservation.AckInput != ackInput || afterObservation.AckReadyCapture == "" || afterObservation.BodyID != archivedObservation.BodyID || afterObservation.ReceiptID != archivedObservation.ReceiptID || afterObservation.ReloadedCapture != reloadCapture.ID {
		t.Fatal("ACK lost report or input evidence", err)
	}
	wantStatus := "running"
	if ackErr != nil {
		wantStatus = "unresolved"
	}
	if afterInput.Stage != "ack_requested" || afterInput.Status != wantStatus {
		t.Fatal("ACK input claimed confirmation", afterInput.Stage, afterInput.Status)
	}
	if _, err := operation.acknowledge(ctx, sendAck); err == nil || ackSubmissions != 1 {
		t.Fatal("ACK input replayed")
	}
	if _, err := RequestReportAcknowledgement(ctx, root, record.OperationID); !errors.Is(err, journal.ErrTransition) {
		t.Fatal("ACK intent replayed", err)
	}
	if ackErr != nil {
		return
	}
	acknowledged.Kind, acknowledged.Sequence = "acknowledged", 11
	frames.frame = makeAckFrame(t, acknowledged)
	frames.frame.SystemTicks = 100 + tickOffset
	if _, err := operation.Observe(ctx); err != nil {
		t.Fatal(err)
	}
	current, err = book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Stage != "acknowledged" {
		t.Fatalf("acknowledged is not cleaned: %+v %v", current, err)
	}
	if _, err := operation.Observe(ctx); !errors.Is(err, journal.ErrTransition) {
		t.Fatalf("unexpected terminal observation: %v", err)
	}
	if ackMode == "retire-before-flush" || strings.HasPrefix(ackMode, "cleanup-") {
		// A valid ACK must let the host unload its queue before the reload
		// which flushes deletion. SavedVariables still contains the original.
		for _, invalid := range []string{wrongSaved, "", `LycheeToolkitDB={schema=2,reports={}}`} {
			if err := os.WriteFile(source, []byte(invalid), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := operation.RetireQueue(ctx); err == nil || len(operationQueue(t, client)) != 1 {
				t.Fatal("changed report retired queue")
			}
		}
		if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
			t.Fatal(err)
		}
		for attempt := 0; attempt < 2; attempt++ {
			revision, err := operation.RetireQueue(ctx)
			if err != nil || revision.Changed != (attempt == 0) || revision.Entries != 0 {
				t.Fatal("pre-flush retirement", revision, err)
			}
		}
		pending, err := book.InspectWork(ctx, record.OperationID)
		var proof reportObservation
		if err != nil || json.Unmarshal(pending.Observation, &proof) != nil || proof.RemovalID != "" || pending.Stage != "acknowledged" || pending.Status != "running" {
			t.Fatal("retirement claimed deletion or completion", err)
		}
		if _, err := operation.ObserveRemoval(ctx); err == nil {
			t.Fatal("original report claimed removed")
		}
		unchanged, err := os.ReadFile(source)
		if err != nil || string(unchanged) != saved {
			t.Fatal("retirement modified SavedVariables", err)
		}
		if strings.HasPrefix(ackMode, "cleanup-") {
			mode := strings.TrimPrefix(ackMode, "cleanup-")
			ready := reentry
			ready.RequestID, ready.ReloadNonce, ready.Sequence = "", "", acknowledged.Sequence+1
			if mode == "unready" {
				ready.InputReady = false
			}
			if mode == "pre-ack" {
				ready.Sequence = acknowledged.Sequence
			}
			frames.frame = makeAckFrame(t, ready)
			frames.frame.SystemTicks = 101 + tickOffset
			cleanCtx, cancelClean := context.WithCancel(ctx)
			defer cancelClean()
			calls := 0
			sendClean := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				calls++
				if target != session.target.Window {
					t.Fatal("wrong cleanup window")
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				intent, err := book.InspectWork(ctx, record.OperationID)
				var durable reportObservation
				if err != nil || json.Unmarshal(intent.Observation, &durable) != nil || durable.CleanupReadyID == "" || len(durable.CleanupNonce) != 32 || command != "/dev bridge clean "+e.RequestID+" "+durable.CleanupNonce {
					t.Fatal("cleanup before durable intent", err)
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				if mode == "cancelled" {
					cancelClean()
					return desktop.InputReceipt{MessagesQueued: 2}, ctx.Err()
				}
				if mode == "partial" {
					return desktop.InputReceipt{MessagesQueued: 3}, partialFailure
				}
				return desktop.InputReceipt{MessagesQueued: 42, SubmissionComplete: true}, nil
			}
			inputReceipt, inputErr := operation.clean(cleanCtx, sendClean)
			if (inputErr == nil) != (mode == "success") {
				t.Fatal("cleanup outcome", inputErr)
			}
			pending, err = book.InspectWork(ctx, record.OperationID)
			if err != nil || json.Unmarshal(pending.Observation, &proof) != nil {
				t.Fatal(err)
			}
			if mode == "unready" || mode == "pre-ack" {
				if proof.CleanupReadyID != "" || proof.CleanupInput != nil || pending.Status != "running" {
					t.Fatal("ineligible cleanup submitted")
				}
				return
			}
			status := "running"
			if mode != "success" {
				status = "unresolved"
			}
			if pending.Stage != "acknowledged" || pending.Status != status || proof.RemovalID != "" || proof.CleanupInput == nil || *proof.CleanupInput != inputReceipt || proof.BodyID != archivedObservation.BodyID || proof.ReceiptID != archivedObservation.ReceiptID {
				t.Fatal("cleanup outcome lost evidence or claimed completion")
			}
			if _, err := operation.clean(ctx, sendClean); err == nil || calls != 1 {
				t.Fatal("cleanup replayed")
			}
			originalObservation := append([]byte(nil), pending.Observation...)
			proof.CleanupReadyID = proof.AckReadyCapture
			wrongEvidence, _ := json.Marshal(proof)
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: pending.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: pending.Status, Observation: wrongEvidence}); err != nil {
				t.Fatal(err)
			}
			if _, err := operation.Complete(ctx); err == nil {
				t.Fatal("foreign readiness provenance accepted")
			}
			pending, err = book.InspectWork(ctx, record.OperationID)
			if err != nil {
				t.Fatal(err)
			}
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: pending.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: pending.Status, Observation: originalObservation}); err != nil {
				t.Fatal(err)
			}
			cleared := bridge.Signal{Schema: "lycheedev.signal.v1", Release: e.Release, Kind: "cleared", SessionNonce: e.SessionNonce, RequestID: e.RequestID, CleanupNonce: proof.CleanupNonce, Character: e.Character, Realm: e.Realm, GUID: input.Load.GUID, Product: e.Product, Build: e.Build, Sequence: 1, RuntimeEpoch: reentry.RuntimeEpoch + 1}
			for index, mutate := range []func(*bridge.Signal){
				func(s *bridge.Signal) { s.RuntimeEpoch-- },
				func(s *bridge.Signal) { s.RuntimeEpoch++ },
				func(s *bridge.Signal) {
					s.CleanupNonce = strings.Repeat("f", 32)
					if s.CleanupNonce == proof.CleanupNonce {
						s.CleanupNonce = strings.Repeat("e", 32)
					}
				},
				func(s *bridge.Signal) { s.GUID = "Player-1-foreign" },
				func(s *bridge.Signal) { s.RequestID = "OP-foreign" },
				func(s *bridge.Signal) { s.InputReady = true },
			} {
				wrong := cleared
				mutate(&wrong)
				frames.frame = makeAckFrame(t, wrong)
				frames.frame.SystemTicks = int64(102+index) + tickOffset
				if _, err := operation.Complete(ctx); err == nil {
					t.Fatal("invalid cleanup handoff completed", index)
				}
				if session.ready.RuntimeEpoch != reentry.RuntimeEpoch {
					t.Fatal("invalid cleanup changed runtime")
				}
			}
			frames.frame = makeAckFrame(t, cleared)
			frames.frame.SystemTicks = 110 + tickOffset
			if _, err := operation.Complete(ctx); err == nil {
				t.Fatal("cleared QR substituted for disk deletion")
			}
			if session.ready.RuntimeEpoch != cleared.RuntimeEpoch {
				t.Fatal("cleanup runtime not adopted")
			}
			confirmed, err := book.InspectWork(ctx, record.OperationID)
			var cleanupProof reportObservation
			if err != nil || json.Unmarshal(confirmed.Observation, &cleanupProof) != nil || cleanupProof.ClearedID == "" || cleanupProof.RemovalID != "" || confirmed.Stage != "acknowledged" {
				t.Fatal("cleanup proof lost or claimed completion", err)
			}
			// Simulate interruption between committed handoff and local adoption.
			session.ready = reentry
			if err := operation.check(ctx); err == nil {
				t.Fatal("stale runtime accepted after cleanup handoff")
			}
			frames.frame = makeAckFrame(t, cleared)
			frames.frame.SystemTicks = 111 + tickOffset
			if _, err := operation.Complete(ctx); err == nil {
				t.Fatal("reconciled QR substituted for disk deletion")
			}
			reconciled, err := book.InspectWork(ctx, record.OperationID)
			if err != nil || reconciled.Generation != confirmed.Generation || session.ready.RuntimeEpoch != cleared.RuntimeEpoch {
				t.Fatal("cleanup reconciliation rewrote proof", err)
			}
			if err := os.WriteFile(source, []byte(`LycheeToolkitDB={schema=1,reports={}}`), 0600); err != nil {
				t.Fatal(err)
			}
			frames.frame = makeAckFrame(t, cleared)
			frames.frame.SystemTicks = 112 + tickOffset
			completed, err := operation.Complete(ctx)
			if err != nil || completed.Stage != "cleaned" || completed.Status != "completed" || operation.run != nil {
				t.Fatal("cleanup lifecycle incomplete", completed.Stage, err)
			}
			if _, err := ReleaseCompletedProbe(ctx, root, record.OperationID); err != nil {
				t.Fatal("cleanup terminal recovery", err)
			}
			if calls != 1 {
				t.Fatal("cleanup reconciliation sent input")
			}
		}
		return
	}
	for _, invalid := range []string{saved, "", `LycheeToolkitDB={schema=2,reports={}}`, `LycheeToolkitDB={schema=1,reports={}};error("injected")`, fmt.Sprintf(`LycheeToolkitDB={schema=1,reports={[%q]=false}}`, e.RequestID)} {
		if err := os.WriteFile(source, []byte(invalid), 0600); err != nil {
			t.Fatal(err)
		}
		if capture, err := operation.ObserveRemoval(ctx); err == nil || capture.ID != "" {
			t.Fatalf("invalid removal archived: %q %v", invalid, err)
		}
	}
	removed := `LycheeToolkitDB={schema=1,reports={["foreign-request"]={receipt="untouched",body="data"}}}`
	if err := os.WriteFile(source, []byte(removed), 0600); err != nil {
		t.Fatal(err)
	}
	capture, err := operation.ObserveRemoval(ctx)
	if err != nil {
		t.Fatal(err)
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	_, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, capture.ID, bridge.SavedStateLimits().FileBytes)
	if err != nil || string(raw) != removed {
		t.Fatal("removal evidence did not retain full original file", err)
	}
	current, err = book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Stage != "acknowledged" || current.Status != "running" {
		t.Fatal("disk absence claimed terminal cleanup", err)
	}
	var removal reportObservation
	if err := json.Unmarshal(current.Observation, &removal); err != nil || removal.RemovalID != capture.ID || len(removal.RemovalSHA256) != 64 {
		t.Fatal("missing removal provenance", err)
	}
	unchanged, err := os.ReadFile(source)
	if err != nil || string(unchanged) != removed || len(operationQueue(t, client)) != 1 {
		t.Fatal("observation changed saved variables or queue", err)
	}
	_, foreign, err := probeDefinition(record)
	if err != nil {
		t.Fatal(err)
	}
	foreign.RequestID = "REQ-foreign"
	addon := filepath.Join(client, "Interface", "AddOns", "Lychee Dev")
	if _, err := delivery.ChangeProbeQueue(ctx, addon, foreign, false); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := operation.RetireQueue(ctx); err == nil || len(operationQueue(t, client)) != 2 {
		t.Fatal("old absence evidence retired a reappeared report")
	}
	if err := os.WriteFile(source, []byte(removed), 0600); err != nil {
		t.Fatal(err)
	}
	queuePath := filepath.Join(addon, "Bridge", "Definitions.lua")
	originalQueue, err := os.ReadFile(queuePath)
	if err != nil {
		t.Fatal(err)
	}
	conflicting := operationQueue(t, client)
	for i := range conflicting {
		if conflicting[i].RequestID == e.RequestID {
			conflicting[i].Code = "return 99"
			conflicting[i].Code += " -- changed"
		}
	}
	rawQueue, err := bridge.EncodeProbeQueue(conflicting)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(queuePath, rawQueue, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := operation.RetireQueue(ctx); !errors.Is(err, delivery.ErrQueueConflict) {
		t.Fatal("changed definition retired", err)
	}
	current, err = book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(current.Observation, &removal); err != nil || removal.QueueRetirement == nil || removal.QueueRetirement.Revision.SHA256 != "" {
		t.Fatal("retirement intent was not retained on conflict", err)
	}
	if err := os.WriteFile(queuePath, originalQueue, 0600); err != nil {
		t.Fatal(err)
	}
	var retiredGeneration int64
	for attempt := 0; attempt < 2; attempt++ {
		revision, err := operation.RetireQueue(ctx)
		if err != nil || revision.Changed != (attempt == 0) || revision.Entries != 1 {
			t.Fatalf("retirement %d: %+v %v", attempt, revision, err)
		}
		current, err = book.InspectWork(ctx, record.OperationID)
		if err != nil || current.Stage != "acknowledged" || current.Status != "running" {
			t.Fatal("queue removal claimed game cleanup", err)
		}
		if attempt != 0 && current.Generation != retiredGeneration {
			t.Fatal("retirement retry churned generation")
		}
		retiredGeneration = current.Generation
	}
	remaining := operationQueue(t, client)
	if len(remaining) != 1 || remaining[0] != foreign {
		t.Fatal("foreign queue entry modified")
	}
	// Simulate loss of the completion write after the atomic queue replacement.
	if err := json.Unmarshal(current.Observation, &removal); err != nil {
		t.Fatal(err)
	}
	removal.QueueRetirement.Revision = delivery.QueueRevision{}
	interrupted, _ := json.Marshal(removal)
	if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: current.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: "unresolved", Observation: interrupted}); err != nil {
		t.Fatal(err)
	}
	if revision, err := operation.RetireQueue(ctx); err != nil || revision.Changed || revision.Entries != 1 {
		t.Fatalf("interrupted retirement: %+v %v", revision, err)
	}
	current, err = book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(current.Observation, &removal); err != nil || removal.QueueRetirement == nil || removal.QueueRetirement.Revision.SHA256 == "" || current.Stage != "acknowledged" {
		t.Fatal("interrupted retirement did not reconcile", err)
	}
	if _, err := operation.Complete(ctx); err == nil {
		t.Fatal("completed without issued cleanup challenge")
	}
	command, err := operation.PrepareCompletion(ctx)
	if err != nil || !strings.HasPrefix(command, "/dev bridge verify "+e.RequestID+" ") {
		t.Fatalf("cleanup challenge: %q %v", command, err)
	}
	issued, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	if repeated, err := operation.PrepareCompletion(ctx); err != nil || repeated != command {
		t.Fatal("challenge changed on retry", err)
	}
	stable, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || stable.Generation != issued.Generation {
		t.Fatal("challenge retry churned generation", err)
	}
	if err := json.Unmarshal(stable.Observation, &removal); err != nil {
		t.Fatal(err)
	}
	cleared := bridge.Signal{Schema: "lycheedev.signal.v1", Release: e.Release, Kind: "cleared", SessionNonce: e.SessionNonce, RequestID: e.RequestID, CleanupNonce: removal.CleanupNonce, Character: e.Character, Realm: e.Realm, GUID: input.Load.GUID, Product: e.Product, Build: e.Build, Sequence: 2, RuntimeEpoch: reentry.RuntimeEpoch}
	for index, epoch := range []uint64{0, reentry.RuntimeEpoch - 1, reentry.RuntimeEpoch + 1} {
		invalid := cleared
		invalid.RuntimeEpoch = epoch
		frames.frame = makeAckFrame(t, invalid)
		frames.frame.SystemTicks = int64(101+index) + tickOffset
		if _, err := operation.Complete(ctx); err == nil {
			t.Fatal("manual completion accepted foreign epoch", epoch)
		}
	}
	wrong := cleared
	wrong.CleanupNonce = "f" + cleared.CleanupNonce[1:]
	if wrong.CleanupNonce == cleared.CleanupNonce {
		wrong.CleanupNonce = "e" + cleared.CleanupNonce[1:]
	}
	frames.frame = makeAckFrame(t, wrong)
	frames.frame.SystemTicks = 104 + tickOffset
	if _, err := operation.Complete(ctx); err == nil {
		t.Fatal("wrong challenge completed operation")
	}
	wrong = cleared
	wrong.GUID = "Player-1-other"
	frames.frame = makeAckFrame(t, wrong)
	frames.frame.SystemTicks = 105 + tickOffset
	if _, err := operation.Complete(ctx); err == nil || err.Error() != "live.cleanup_guid_mismatch" {
		t.Fatal("wrong GUID completed operation", err)
	}
	// Reappearing queue entries are rejected, not silently deleted by Complete.
	_, original, err := probeDefinition(record)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := delivery.ChangeProbeQueue(ctx, addon, original, false); err != nil {
		t.Fatal(err)
	}
	if _, err := operation.Complete(ctx); !errors.Is(err, delivery.ErrQueueConflict) || len(operationQueue(t, client)) != 2 {
		t.Fatal("completion removed a reappeared entry", err)
	}
	if _, err := delivery.ChangeProbeQueue(ctx, addon, original, true); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(source, []byte(saved), 0600); err != nil {
		t.Fatal(err)
	}
	frames.frame = makeAckFrame(t, cleared)
	frames.frame.SystemTicks = 106 + tickOffset
	if _, err := operation.Complete(ctx); err == nil {
		t.Fatal("reappeared disk report completed operation")
	}
	if err := os.WriteFile(source, []byte(removed), 0600); err != nil {
		t.Fatal(err)
	}
	frames.frame = makeAckFrame(t, cleared)
	frames.frame.SystemTicks = 107 + tickOffset
	ownerPath := filepath.Join(client, "Interface", "AddOns", ".lycheedev-window-owners", fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	ownerBytes, err := os.ReadFile(ownerPath)
	if err != nil {
		t.Fatal(err)
	}
	completed, err := operation.Complete(ctx)
	if err != nil || completed.Stage != "cleaned" || completed.Status != "completed" {
		t.Fatalf("completion: %+v %v", completed, err)
	}
	if err := json.Unmarshal(completed.Observation, &removal); err != nil || removal.ClearedID == "" {
		t.Fatal("missing game cleanup evidence", err)
	}
	if frames.closed {
		t.Fatal("completion closed borrowed capture")
	}
	if _, err := operation.Complete(ctx); err == nil {
		t.Fatal("completed handle reused")
	}
	// Restore only the isolated fixture marker to model a process exiting after
	// the terminal DB commit but before durable owner retirement.
	if err := os.WriteFile(ownerPath, ownerBytes, 0600); err != nil {
		t.Fatal(err)
	}
	for attempt := 0; attempt < 2; attempt++ {
		// Cross the real command boundary without importing command (which
		// depends on live). The fixture contains no live game installation.
		command := exec.Command("go", "run", "../../cmd/lycheedev", "live", "resume", record.OperationID, "--home", root, "--format=json")
		if node, launcher := os.Getenv("LYCHEEDEV_RECOVERY_NODE"), os.Getenv("LYCHEEDEV_RECOVERY_LAUNCHER"); node != "" || launcher != "" {
			for _, path := range []string{node, launcher} {
				info, err := os.Stat(path)
				if !filepath.IsAbs(path) || err != nil || !info.Mode().IsRegular() {
					t.Fatalf("invalid installed CLI path %q: %v", path, err)
				}
			}
			command = exec.Command(node, launcher, "live", "resume", record.OperationID, "--home", root, "--format=json")
			t.Logf("using installed recovery launcher: %s", launcher)
		}
		output, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("CLI terminal recovery: %v %s", err, output)
		}
		var envelope struct {
			OK          bool     `json:"ok"`
			OperationID string   `json:"operationId"`
			Result      Outcome  `json:"result"`
			Warnings    []string `json:"warnings"`
		}
		if err := json.Unmarshal(output, &envelope); err != nil || !envelope.OK || envelope.OperationID != record.OperationID {
			t.Fatalf("resume output: %s %v", output, err)
		}
		released := envelope.Result
		latest, err := book.InspectWork(ctx, record.OperationID)
		if err != nil || latest.Generation != completed.Generation || !released.Complete || released.Report.State != "verified" || released.Cleanup != "complete" {
			t.Fatal("terminal retirement did not reconcile idempotently", err)
		}
	}
	// A task-correlated reentry receipt cannot masquerade as the generic ready
	// handshake for a different operation. Model a fresh explicit ready display.
	if _, err := session.PrepareProbe(ctx, root, record.Intent.Snapshot, input.Load.Account, input.Code); err == nil {
		t.Fatal("reused task-specific reentry for a new probe")
	}
	newReady := session.ready
	newReady.RequestID, newReady.ReloadNonce = "", ""
	newReady.Sequence++
	nextExpected := input.Expected
	nextExpected.Kind, nextExpected.RequestID, nextExpected.ReloadNonce = "ready", "", ""
	nextExpected.AfterSequence, nextExpected.RequireInputReady = 0, true
	nextFrames := &sessionFixtureFrames{ackFrames: ackFrames{frame: makeAckFrame(t, newReady)}}
	nextSession, err := observeWindowSession(ctx, session.target, nextExpected, nextFrames, session.confirm)
	if err != nil {
		t.Fatal(err)
	}
	defer nextSession.Close()
	if _, err := nextSession.PrepareProbe(ctx, root, record.Intent.Snapshot, input.Load.Account, input.Code); err != nil {
		t.Fatal("completed owner prevented next operation", err)
	}
	if _, err := ReleaseCompletedProbe(ctx, root, record.OperationID); !errors.Is(err, journal.ErrBusy) {
		t.Fatal("old retirement cleared newer owner", err)
	}
}
func TestProbeQueueIntentBeforeFileEffect(t *testing.T) {
	root, client, book, record, _ := probeOperationFixture(t)
	parent := filepath.Join(client, "Interface", "AddOns")
	target := filepath.Join(parent, "Lychee Dev")
	lease, err := vault.AcquireLease(context.Background(), filepath.Join(parent, ".lycheedev-locks"), "installation:"+strings.ToLower(target))
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := PrepareOperationQueue(ctx, root, record.OperationID); done <- err }()
	deadline := time.After(3 * time.Second)
	for {
		observed, err := book.InspectWork(context.Background(), record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
		if observed.Stage == "load_requested" {
			break
		}
		select {
		case <-deadline:
			t.Fatal("intent not recorded before waiting for installation lock")
		case <-time.After(10 * time.Millisecond):
		}
	}
	if len(operationQueue(t, client)) != 0 {
		t.Fatal("file changed before lock acquisition")
	}
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel: %v", err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	revision, err := PrepareOperationQueue(context.Background(), root, record.OperationID)
	if err != nil || !revision.Changed || revision.Entries != 1 {
		t.Fatalf("resume: %+v %v", revision, err)
	}
	observed, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || observed.Stage != "load_requested" {
		t.Fatalf("queue write claimed game load: %+v %v", observed, err)
	}
}
func TestProbePrepareConcurrentRetryAndLoadedEvidence(t *testing.T) {
	root, client, book, record, input := probeOperationFixture(t)
	var wg sync.WaitGroup
	failures := make(chan error, 2)
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := PrepareOperationQueue(context.Background(), root, record.OperationID)
			failures <- err
		}()
	}
	wg.Wait()
	close(failures)
	for err := range failures {
		if err != nil {
			t.Fatal(err)
		}
	}
	definitions := operationQueue(t, client)
	if len(definitions) != 1 || definitions[0].Code != string(input.Code) {
		t.Fatal("queue retry changed request")
	}
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: input.Expected.Release, Kind: "loaded", SessionNonce: input.Expected.SessionNonce, ReloadNonce: input.Load.ReloadNonce, RequestID: input.Expected.RequestID, Character: input.Expected.Character, Realm: input.Expected.Realm, Product: input.Expected.Product, Build: input.Expected.Build, Sequence: 2, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
	for _, variant := range []string{"missing", "wrong-reload", "wrong-code", "stale"} {
		changed := signal
		feed := &ackFrames{}
		if variant == "wrong-reload" {
			changed.ReloadNonce = strings.Repeat("c", 32)
		}
		if variant == "wrong-code" {
			changed.CodeAdler32 = "00000000"
		}
		if variant == "stale" {
			changed.Sequence = 1
		}
		if variant != "missing" {
			feed.frame = makeAckFrame(t, changed)
		}
		if _, err := ObserveOperationLoaded(context.Background(), root, record.OperationID, bridge.ObserveSignals(feed)); err == nil {
			t.Fatalf("accepted %s", variant)
		}
		observed, err := book.InspectWork(context.Background(), record.OperationID)
		if err != nil || observed.Stage != "load_requested" {
			t.Fatal("invalid signal advanced operation", err)
		}
	}
	capture, err := ObserveOperationLoaded(context.Background(), root, record.OperationID, bridge.ObserveSignals(&ackFrames{frame: makeAckFrame(t, signal)}))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := evidence.InspectEvidence(context.Background(), root, capture.ID, true); err != nil {
		t.Fatal(err)
	}
	observed, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || observed.Stage != "loaded" {
		t.Fatalf("%+v %v", observed, err)
	}
	if _, err := PrepareOperationQueue(context.Background(), root, record.OperationID); !errors.Is(err, journal.ErrTransition) {
		t.Fatalf("reprepared loaded operation: %v", err)
	}
	if len(operationQueue(t, client)) != 1 {
		t.Fatal("load observation removed request")
	}
}
func TestProbeInstallIdentityChangedBeforePreparation(t *testing.T) {
	root, client, book, record, _ := probeOperationFixture(t)
	testkit.WriteFile(t, filepath.Join(client, "version.txt"), "12.1.0.99999\n")
	if _, err := PrepareOperationQueue(context.Background(), root, record.OperationID); err == nil {
		t.Fatal("different build accepted")
	}
	observed, err := book.InspectWork(context.Background(), record.OperationID)
	if err != nil || observed.Stage != "prepared" || len(operationQueue(t, client)) != 0 {
		t.Fatalf("preflight failure changed state: %+v %v", observed, err)
	}
}
