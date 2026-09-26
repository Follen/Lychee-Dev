package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type BugsRequest struct {
	Session string
	Account string
	Request string
	Count   int
}

func (r BugsRequest) Validate() error {
	if strings.TrimSpace(r.Session) == "" || len(r.Session) > 128 {
		return errors.New("live.session_required")
	}
	if strings.TrimSpace(r.Request) == "" || len(r.Request) > 128 {
		return errors.New("live.request_key_invalid")
	}
	if r.Count < 1 || r.Count > 100 {
		return errors.New("live.bugs_count_invalid")
	}
	if r.Account != "" {
		return validateReportAccount(r.Account)
	}
	return nil
}

type faultObservation struct {
	Schema          string                `json:"schema"`
	ReportedCapture string                `json:"reportedCapture,omitempty"`
	ReloadedCapture string                `json:"reloadedCapture,omitempty"`
	DispatchInput   *desktop.InputReceipt `json:"dispatchInput,omitempty"`
	FlushInput      *desktop.InputReceipt `json:"flushInput,omitempty"`
}

type faultOperation struct {
	session  *WindowSession
	root, id string
	run      *journal.WindowRun
	metadata *vault.Metadata
}

func Bugs(ctx context.Context, root string, request BugsRequest) (Outcome, error) {
	if err := request.Validate(); err != nil {
		return Outcome{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	session, snapshot, err := reconnectSession(ctx, root, request.Session, nativeIO())
	if err != nil {
		return Outcome{}, err
	}
	defer session.Close()
	record, err := session.prepareFaults(ctx, root, snapshot, request)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	if record.Stage == "verified" || record.Stage == "cleaned" {
		return finishOutcome(ctx, root, record, nil)
	}
	op, err := session.openFaultOperation(ctx, root, record.OperationID)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	defer op.close()
	record, err = op.execute(ctx, desktop.QueuePreparedCommand)
	return finishOutcome(ctx, root, record, err)
}

func (s *WindowSession) prepareFaults(ctx context.Context, root, snapshot string, request BugsRequest) (journal.WorkRecord, error) {
	account, err := resolveReportAccount(ctx, s.target.Client.Directory, s.ready.Character, s.ready.Realm, request.Account)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	var entropy [32]byte
	if _, err := rand.Read(entropy[:]); err != nil {
		return journal.WorkRecord{}, err
	}
	binding, err := SaveWindowSession(ctx, root, snapshot, s)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	ready := s.ready
	input := ReportIntent{Schema: "lycheedev.report-intent.v1", Revision: "builtin:bugs", Binding: binding.ID,
		Expected: bridge.SignalExpectation{Kind: "reported", Release: ready.Release, SessionNonce: ready.SessionNonce,
			RequestID: "REQ-" + hex.EncodeToString(entropy[:16]), Character: ready.Character, Realm: ready.Realm,
			Product: ready.Product, Build: ready.Build, AfterSequence: ready.Sequence},
		Load: &ProbeLoadIntent{Installation: s.target.Client.Directory, Account: account, GUID: ready.GUID, ReloadNonce: hex.EncodeToString(entropy[16:])}}
	raw, _ := json.Marshal(input)
	resource := windowResource(s.target)
	digestRaw, _ := json.Marshal(struct {
		Resource string `json:"resource"`
		Snapshot string `json:"snapshot"`
		Account  string `json:"account"`
		Count    int    `json:"count"`
	}{resource, snapshot, account, request.Count})
	digest := sha256.Sum256(digestRaw)
	key := sha256.Sum256([]byte(resource + "\x00" + request.Request))
	intent := journal.WorkIntent{Kind: "faults", Resource: resource, Snapshot: snapshot, Session: ready.SessionNonce, Request: raw,
		RequestKey: "live-" + hex.EncodeToString(key[:]), RequestDigest: hex.EncodeToString(digest[:]), Goal: "verified"}
	// Count is business input, not transport metadata; retain it alongside the
	// report intent so resume never accepts a changed value.
	var envelope map[string]any
	_ = json.Unmarshal(intent.Request, &envelope)
	envelope["count"] = request.Count
	intent.Request, _ = json.Marshal(envelope)
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		if _, err := readSessionEvidence(ctx, store, metadata, binding); err != nil {
			return journal.WorkRecord{}, err
		}
		return journal.OpenBook(metadata).BeginWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, intent)
	})
}

func faultInput(record journal.WorkRecord) (ReportIntent, int, error) {
	input, err := reportInput(record)
	if err != nil || record.Intent.Kind != "faults" || input.Revision != "builtin:bugs" || input.Load == nil {
		return input, 0, errors.Join(err, errors.New("live.invalid_bugs_intent"))
	}
	var raw struct {
		Count int `json:"count"`
	}
	if json.Unmarshal(record.Intent.Request, &raw) != nil || raw.Count < 1 || raw.Count > 100 {
		return input, 0, errors.New("live.invalid_bugs_intent")
	}
	return input, raw.Count, nil
}

func (s *WindowSession) openFaultOperation(ctx context.Context, root, id string) (op *faultOperation, err error) {
	store, err := vault.OpenStore(root)
	if err != nil {
		return nil, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return nil, err
	}
	op = &faultOperation{session: s, root: store.Root(), id: id, metadata: metadata}
	defer func() {
		if err != nil {
			err = errors.Join(err, op.close())
		}
	}()
	record, err := journal.OpenBook(metadata).InspectWork(ctx, id)
	if err != nil {
		return nil, err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return nil, err
	}
	bound, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil || bound.Target != s.target || bound.Ready.GUID != input.Load.GUID {
		return nil, errors.Join(err, errors.New("live.bugs_binding_mismatch"))
	}
	op.run, err = journal.OpenBook(metadata).AcquireWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, id)
	if err != nil {
		return nil, err
	}
	return op, op.check(ctx)
}

func (p *faultOperation) close() error {
	if p == nil {
		return nil
	}
	var err error
	if p.run != nil {
		err = p.run.Close()
		p.run = nil
	}
	if p.metadata != nil {
		err = errors.Join(err, p.metadata.Close())
		p.metadata = nil
	}
	return err
}

func (p *faultOperation) check(ctx context.Context) error {
	record, err := p.run.Check(ctx)
	if err != nil {
		return err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return err
	}
	if err := p.session.confirm(ctx, p.session.target); err != nil {
		return err
	}
	ready := p.session.ready
	if ready.Release != input.Expected.Release || ready.SessionNonce != input.Expected.SessionNonce || ready.Character != input.Expected.Character || ready.Realm != input.Expected.Realm || ready.Product != input.Expected.Product || ready.Build != input.Expected.Build || ready.GUID != input.Load.GUID {
		return errors.New("live.bugs_session_mismatch")
	}
	return nil
}

// unsentAckIntent reports durable proof that a persisted ack intent queued no
// keyboard message at all. Only such a receipt makes re-sending safe; a missing
// receipt leaves the delivered state unknowable and stays observe-only.
func unsentAckIntent(record journal.WorkRecord) bool {
	if record.Stage != "ack_requested" {
		return false
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return false
	}
	return observed.AckInput != nil && observed.AckInput.MessagesQueued == 0 && !observed.AckInput.SubmissionComplete
}

func (p *faultOperation) execute(ctx context.Context, send preparedInput) (journal.WorkRecord, error) {	for step := 0; step < 8; step++ {
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return record, err
		}
		if operationGoalReached(record) {
			return record, nil
		}
		switch record.Stage {
		case "prepared":
			err = p.sendBugs(ctx, send)
		case "dispatch_requested":
			err = p.observeReported(ctx)
		case "reported":
			err = p.sendFlush(ctx, send)
		case "flush_requested":
			err = p.observeAndArchive(ctx)
		case "persisted":
			err = p.promotePersisted(ctx)
		case "verified":
			err = p.sendAck(ctx, send)
		case "ack_requested":
			if unsentAckIntent(record) {
				// The persisted receipt proves no keyboard message reached the
				// window, so re-preparing and re-sending cannot replay input.
				err = p.sendAck(ctx, send)
				break
			}
			wait, cancel := context.WithTimeout(ctx, 15*time.Second)
			_, err = p.observeAcknowledgement(wait)
			cancel()
		case "acknowledged":
			return p.finalize(ctx)
		case "cleaned":
			return record, nil
		default:
			err = journal.ErrTransition
		}
		if err != nil {
			return latestReloadRecord(ctx, p.metadata, p.id, err)
		}
	}
	return journal.WorkRecord{}, errors.New("live.bugs_step_limit")
}

func (p *faultOperation) sendStage(ctx context.Context, send preparedInput, from, to, command string, receiptField func(*faultObservation, *desktop.InputReceipt)) error {
	if send == nil {
		return errors.New("live.input_sender_missing")
	}
	prepared, first := false, false
	receipt, sendErr := send(ctx, p.session.target.Window, func(ctx context.Context) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		var observed faultObservation
		if from == "prepared" {
			observed.Schema = "lycheedev.fault-observation.v1"
		} else if err = json.Unmarshal(record.Observation, &observed); err != nil {
			return "", err
		}
		raw, _ := json.Marshal(observed)
		if err = journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: from, Stage: to, Status: "running", Observation: raw}); err != nil {
			return "", err
		}
		prepared, first = true, true
		return command, nil
	}, func(ctx context.Context) error {
		if err := p.check(ctx); err != nil {
			return err
		}
		if first {
			if err := p.session.reader.RequireFreshSignal(); err != nil {
				return err
			}
			first = false
		}
		return nil
	})
	if !prepared {
		return sendErr
	}
	persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	record, err := journal.OpenBook(p.metadata).InspectWork(persist, p.id)
	if err != nil {
		return errors.Join(sendErr, err)
	}
	var observed faultObservation
	if err = json.Unmarshal(record.Observation, &observed); err != nil {
		return errors.Join(sendErr, err)
	}
	receiptField(&observed, &receipt)
	raw, _ := json.Marshal(observed)
	status := "running"
	if sendErr != nil {
		status = "unresolved"
	}
	err = journal.OpenBook(p.metadata).AdvanceStage(persist, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: to, Stage: to, Status: status, Observation: raw})
	return errors.Join(sendErr, err)
}

func (p *faultOperation) sendBugs(ctx context.Context, send preparedInput) error {
	record, _ := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	input, count, err := faultInput(record)
	if err != nil {
		return err
	}
	return p.sendStage(ctx, send, "prepared", "dispatch_requested", fmt.Sprintf("/dev bridge bugs %s %d", input.Expected.RequestID, count), func(o *faultObservation, r *desktop.InputReceipt) { o.DispatchInput = r })
}

func (p *faultOperation) observeReported(ctx context.Context) error {
	if err := p.check(ctx); err != nil {
		return err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return err
	}
	expected := input.Expected
	expected.Kind = "reported"
	expected.RequireInputReady = false
	wait, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	signal, err := p.session.reader.WaitForSignal(wait, expected)
	if err != nil {
		return err
	}
	if signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes == 0 {
		return errors.New("live.invalid_bugs_report")
	}
	rawSignal, _ := json.Marshal(signal)
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return err
	}
	capture, err := evidence.OpenArchive(store, p.metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(rawSignal), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-game-reported", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
	if err != nil {
		return err
	}
	var observed faultObservation
	if err = json.Unmarshal(record.Observation, &observed); err != nil {
		return err
	}
	observed.ReportedCapture = capture.ID
	raw, _ := json.Marshal(observed)
	return journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "dispatch_requested", Stage: "reported", Status: "running", Observation: raw})
}

func (p *faultOperation) sendFlush(ctx context.Context, send preparedInput) error {
	record, _ := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	input, _, err := faultInput(record)
	if err != nil {
		return err
	}
	return p.sendStage(ctx, send, "reported", "flush_requested", "/dev bridge flush "+input.Expected.RequestID+" "+input.Load.ReloadNonce, func(o *faultObservation, r *desktop.InputReceipt) { o.FlushInput = r })
}

func (p *faultOperation) observeAndArchive(ctx context.Context) error {
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return err
	}
	var observed faultObservation
	if err = json.Unmarshal(record.Observation, &observed); err != nil {
		return err
	}
	expected := input.Expected
	expected.Kind = "ready"
	expected.ReloadNonce = input.Load.ReloadNonce
	expected.AfterSequence = 0
	expected.RuntimeEpoch = p.session.ready.RuntimeEpoch + 1
	expected.RequireInputReady = true
	wait, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	signal, err := p.session.reader.WaitForSignal(wait, expected)
	if err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID {
		return errors.New("live.bugs_reload_identity")
	}
	rawSignal, _ := json.Marshal(signal)
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return err
	}
	archive := evidence.OpenArchive(store, p.metadata)
	capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(rawSignal), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-bugs-reentry", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
	if err != nil {
		return err
	}
	ref, reportedRaw, err := archive.FetchCapture(ctx, observed.ReportedCapture, 4096)
	if err != nil {
		return err
	}
	wantReported := evidence.Provenance{Kind: "decoded-game-reported", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
	if ref.Provenance != wantReported || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return errors.New("live.bugs_report_evidence_mismatch")
	}
	reported, err := bridge.ParseSignal(reportedRaw)
	if err != nil {
		return err
	}
	installed, err := ReadInstalledReport(ctx, input.Load.Installation, input.Load.Account, nil, input.Expected)
	if err != nil {
		return err
	}
	// The wire receipt omits the actor identity, so both observations are
	// compared on the fields the wire actually carries.
	if !sameReceiptIdentity(installed.Report.Receipt, reported) {
		return errors.New("live.persisted_receipt_mismatch")
	}
	pair, err := archive.CommitReport(ctx, installed.Report.ReceiptBytes, installed.Report.Body, nil, input.Expected, record.OperationID, record.Intent.Snapshot)
	if err != nil {
		return err
	}
	observed.ReloadedCapture = capture.ID
	reportObs := reportObservation{Schema: "lycheedev.report-observation.v1", BodyID: pair.Body.ID, ReceiptID: pair.Receipt.ID, SourcePath: installed.Path, SourceSHA256: installed.FileSHA256, ReloadedCapture: capture.ID}
	raw, _ := json.Marshal(reportObs)
	if err = journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "flush_requested", Stage: "persisted", Status: "running", Observation: json.RawMessage(raw)}); err != nil {
		return err
	}
	record, err = journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	if err = journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "persisted", Stage: "verified", Status: "running", Observation: raw}); err != nil {
		return err
	}
	p.session.ready = signal
	return nil
}

func (p *faultOperation) sendAck(ctx context.Context, send preparedInput) error {
	if send == nil {
		return errors.New("live.input_sender_missing")
	}
	prepared, first := false, false
	receipt, sendErr := send(ctx, p.session.target.Window, func(ctx context.Context) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		input, _, err := faultInput(record)
		if err != nil {
			return "", err
		}
		if record.Stage != "verified" && !unsentAckIntent(record) {
			return "", journal.ErrTransition
		}
		// The ack input guard demands a freshly observed signal, but a resumed
		// process starts with an observation-free reader. Wait for the current
		// input-ready receipt — anchored on the archived reload readiness —
		// before queueing the command, exactly like the probe ack path.
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.RuntimeEpoch, expected.RequireInputReady = p.session.ready.RuntimeEpoch, true
		expected.AfterSequence = p.session.ready.Sequence - 1
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			if ctx.Err() == nil && (errors.Is(err, context.DeadlineExceeded) || errors.Is(err, io.EOF)) {
				return "", errors.Join(ErrAckReadinessPending, err)
			}
			return "", err
		}
		generic := signal.RequestID == "" && signal.ReloadNonce == ""
		correlated := signal.RequestID == input.Expected.RequestID && signal.ReloadNonce == input.Load.ReloadNonce
		if (!generic && !correlated) || signal.GUID != input.Load.GUID || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.CleanupNonce != "" {
			return "", errors.New("live.invalid_ack_readiness")
		}
		p.session.ready = signal
		var report bridge.VerifiedReport
		if record.Stage == "verified" {
			report, err = RequestReportAcknowledgement(ctx, p.root, p.id)
		} else {
			// Recovery of an intent whose receipt proves no message was queued:
			// the durable verified report is the same evidence, and the stage
			// machine stays at ack_requested for the post-send persistence.
			_, err = vault.ReadWorkspace(ctx, p.root, func(store *vault.Store, metadata *vault.Metadata) (struct{}, error) {
				var observed reportObservation
				if err := json.Unmarshal(record.Observation, &observed); err != nil {
					return struct{}{}, err
				}
				if observed.Schema != "lycheedev.report-observation.v1" {
					return struct{}{}, errors.New("live.invalid_report_observation")
				}
				var err error
				report, err = evidence.OpenArchive(store, metadata).ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, p.id, record.Intent.Snapshot)
				return struct{}{}, err
			})
		}
		if err != nil {
			return "", err
		}
		prepared, first = true, true
		return fmt.Sprintf("/dev bridge bugs-ack %s %d", input.Expected.RequestID, report.Receipt.Sequence), nil
	}, func(ctx context.Context) error {
		if err := p.check(ctx); err != nil {
			return err
		}
		if first {
			if err := p.session.reader.RequireFreshSignal(); err != nil {
				return err
			}
			first = false
		}
		return nil
	})
	if !prepared {
		return sendErr
	}
	persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	record, err := journal.OpenBook(p.metadata).InspectWork(persist, p.id)
	if err != nil {
		return errors.Join(sendErr, err)
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return errors.Join(sendErr, err)
	}
	observed.AckInput = &receipt
	raw, _ := json.Marshal(observed)
	status := "running"
	if sendErr != nil {
		status = "unresolved"
	}
	err = journal.OpenBook(p.metadata).AdvanceStage(persist, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "ack_requested", Stage: "ack_requested", Status: status, Observation: raw})
	return errors.Join(sendErr, err)
}

func (p *faultOperation) promotePersisted(ctx context.Context) error {
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return err
	}
	var observed reportObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return err
	}
	if _, err := evidence.OpenArchive(store, p.metadata).ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, nil, input.Expected, record.OperationID, record.Intent.Snapshot); err != nil {
		return err
	}
	return journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "persisted", Stage: "verified", Status: "running", Observation: record.Observation})
}

func (p *faultOperation) observeAcknowledgement(ctx context.Context) (evidence.CaptureRef, error) {
	return observeReportAcknowledgement(ctx, p.root, p.id, p.session.reader.WaitForSignal)
}

func (p *faultOperation) finalize(ctx context.Context) (journal.WorkRecord, error) {
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return record, err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return record, err
	}
	var observed reportObservation
	if err = json.Unmarshal(record.Observation, &observed); err != nil {
		return record, err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return record, err
	}
	if _, err = acknowledgedReport(ctx, evidence.OpenArchive(store, p.metadata), record, input, observed); err != nil {
		return record, err
	}
	book := journal.OpenBook(p.metadata)
	if err = book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "cleaned", Status: "completed", Observation: record.Observation}); err != nil {
		return record, err
	}
	completed, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return completed, err
	}
	if err = p.run.Close(); err != nil {
		return completed, err
	}
	p.run = nil
	err = book.RetireWindowWork(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, p.id)
	return completed, err
}

func resumeFaults(ctx context.Context, root, id string, region image.Rectangle, capture func(context.Context, ClientWindow, image.Rectangle) (sessionFrames, error), confirm func(context.Context, ClientWindow) error, send preparedInput) (journal.WorkRecord, error) {
	record, err := InspectOperation(ctx, root, id)
	if err != nil || record.Stage == "cleaned" {
		return record, err
	}
	input, _, err := faultInput(record)
	if err != nil {
		return record, err
	}
	bound, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil {
		return record, err
	}
	if err = confirm(ctx, bound.Target); err != nil {
		return record, err
	}
	frames, err := capture(ctx, bound.Target, region)
	if err != nil {
		return record, err
	}
	ready := bound.Ready
	if record.Stage == "persisted" || record.Stage == "verified" || record.Stage == "ack_requested" || record.Stage == "acknowledged" {
		ready, err = readFaultReloadAnchor(ctx, root, record, input)
		if err != nil {
			return record, err
		}
	}
	session := newWindowSession(bound.Target, region, ready, bridge.ObserveSignals(frames), frames, confirm)
	defer session.Close()
	op, err := session.openFaultOperation(ctx, root, id)
	if err != nil {
		return record, err
	}
	defer op.close()
	return op.execute(ctx, send)
}

func readFaultReloadAnchor(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent) (bridge.Signal, error) {
	return vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (bridge.Signal, error) {
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil || observed.ReloadedCapture == "" {
			return bridge.Signal{}, errors.New("live.bugs_reload_evidence_missing")
		}
		ref, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, observed.ReloadedCapture, 4096)
		if err != nil {
			return bridge.Signal{}, err
		}
		want := evidence.Provenance{Kind: "decoded-bugs-reentry", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
		if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return bridge.Signal{}, errors.New("live.bugs_reload_evidence_mismatch")
		}
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			return bridge.Signal{}, err
		}
		expected := input.Expected
		expected.Kind, expected.ReloadNonce, expected.AfterSequence, expected.RequireInputReady = "ready", input.Load.ReloadNonce, 0, true
		if err := signal.Match(expected); err != nil || signal.GUID != input.Load.GUID {
			return bridge.Signal{}, errors.Join(err, errors.New("live.bugs_reload_identity"))
		}
		return signal, nil
	})
}

// ReleaseCompletedFaults closes the same durable-terminal/owner-retirement
// crash window as the probe and standalone reload paths.
func ReleaseCompletedFaults(ctx context.Context, root string, record journal.WorkRecord) (journal.WorkRecord, error) {
	input, _, err := faultInput(record)
	if err != nil || record.Stage != "cleaned" || record.Status != "completed" {
		return record, errors.Join(err, journal.ErrTransition)
	}
	_, err = vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (struct{}, error) {
		return struct{}{}, journal.OpenBook(metadata).RetireWindowWork(ctx, filepath.Join(input.Load.Installation, "Interface", "AddOns"), store.Identity().WorkspaceID, record.OperationID)
	})
	return record, err
}
