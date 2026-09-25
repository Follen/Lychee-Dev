package live

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"image"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type ReloadRequest struct {
	Session string
	Request string
}

func (r ReloadRequest) Validate() error {
	if strings.TrimSpace(r.Session) == "" || len(r.Session) > 128 {
		return errors.New("live.session_required")
	}
	if strings.TrimSpace(r.Request) == "" || len(r.Request) > 128 {
		return errors.New("live.request_key_invalid")
	}
	return nil
}

type standaloneReloadIntent struct {
	Schema       string                   `json:"schema"`
	Binding      string                   `json:"binding"`
	Installation string                   `json:"installation"`
	GUID         string                   `json:"guid"`
	ReloadNonce  string                   `json:"reloadNonce"`
	Expected     bridge.SignalExpectation `json:"expected"`
}

type standaloneReloadObservation struct {
	Schema       string                `json:"schema"`
	Input        *desktop.InputReceipt `json:"input,omitempty"`
	ReadyCapture string                `json:"readyCapture,omitempty"`
}

type standaloneReloadOperation struct {
	session  *WindowSession
	root, id string
	run      *journal.WindowRun
	metadata *vault.Metadata
}

func ReloadClient(ctx context.Context, root string, request ReloadRequest) (Outcome, error) {
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
	record, err := session.prepareStandaloneReload(ctx, root, snapshot, request.Request)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	if record.Stage == "cleaned" {
		return finishOutcome(ctx, root, record, nil)
	}
	operation, err := session.openStandaloneReload(ctx, root, record.OperationID)
	if err != nil {
		return finishOutcome(ctx, root, record, err)
	}
	defer operation.close()
	record, err = operation.execute(ctx, desktop.QueuePreparedCommand)
	return finishOutcome(ctx, root, record, err)
}

func (s *WindowSession) prepareStandaloneReload(ctx context.Context, root, snapshot, requestKey string) (journal.WorkRecord, error) {
	if s == nil || s.closed || s.reader == nil || s.confirm == nil {
		return journal.WorkRecord{}, errors.New("live.session_closed")
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return journal.WorkRecord{}, err
	}
	binding, err := SaveWindowSession(ctx, root, snapshot, s)
	if err != nil {
		return journal.WorkRecord{}, err
	}
	ready := s.ready
	input := standaloneReloadIntent{
		Schema: "lycheedev.reload-intent.v1", Binding: binding.ID, Installation: s.target.Client.Directory,
		GUID: ready.GUID, ReloadNonce: hex.EncodeToString(nonce[:]),
		Expected: bridge.SignalExpectation{Kind: "ready", Release: ready.Release, SessionNonce: ready.SessionNonce,
			RequestID: "RELOAD-" + hex.EncodeToString(nonce[:]), ReloadNonce: hex.EncodeToString(nonce[:]),
			Character: ready.Character, Realm: ready.Realm, Product: ready.Product, Build: ready.Build,
			AfterSequence: 0, RuntimeEpoch: ready.RuntimeEpoch + 1, RequireInputReady: true},
	}
	raw, _ := json.Marshal(input)
	resource := windowResource(s.target)
	digestInput, _ := json.Marshal(struct {
		Resource string `json:"resource"`
		Snapshot string `json:"snapshot"`
		Session  string `json:"session"`
	}{resource, snapshot, ready.SessionNonce})
	digest := sha256.Sum256(digestInput)
	key := sha256.Sum256([]byte(resource + "\x00" + requestKey))
	intent := journal.WorkIntent{Kind: "reload", Resource: resource, Snapshot: snapshot, Session: ready.SessionNonce,
		Request: raw, RequestKey: "live-" + hex.EncodeToString(key[:]), RequestDigest: hex.EncodeToString(digest[:]), Goal: "cleaned"}
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		if _, err := readSessionEvidence(ctx, store, metadata, binding); err != nil {
			return journal.WorkRecord{}, err
		}
		if err := s.confirm(ctx, s.target); err != nil {
			return journal.WorkRecord{}, err
		}
		return journal.OpenBook(metadata).BeginWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, intent)
	})
}

func parseStandaloneReload(record journal.WorkRecord) (standaloneReloadIntent, error) {
	var input standaloneReloadIntent
	if record.Intent.Kind != "reload" || json.Unmarshal(record.Intent.Request, &input) != nil || input.Schema != "lycheedev.reload-intent.v1" || input.Binding == "" || !filepath.IsAbs(input.Installation) || len(input.ReloadNonce) != 32 || input.Expected.RequestID != "RELOAD-"+input.ReloadNonce || input.Expected.ReloadNonce != input.ReloadNonce || input.Expected.Kind != "ready" {
		return input, errors.New("live.invalid_reload_intent")
	}
	if _, err := hex.DecodeString(input.ReloadNonce); err != nil {
		return input, errors.New("live.invalid_reload_intent")
	}
	return input, nil
}

func (s *WindowSession) openStandaloneReload(ctx context.Context, root, id string) (op *standaloneReloadOperation, err error) {
	store, err := vault.OpenStore(root)
	if err != nil {
		return nil, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return nil, err
	}
	op = &standaloneReloadOperation{session: s, root: store.Root(), id: id, metadata: metadata}
	defer func() {
		if err != nil {
			err = errors.Join(err, op.close())
		}
	}()
	record, err := journal.OpenBook(metadata).InspectWork(ctx, id)
	if err != nil {
		return nil, err
	}
	input, err := parseStandaloneReload(record)
	if err != nil {
		return nil, err
	}
	bound, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil || bound.Target != s.target || bound.Record.Snapshot != record.Intent.Snapshot || bound.Ready.GUID != input.GUID {
		return nil, errors.Join(err, errors.New("live.reload_binding_mismatch"))
	}
	op.run, err = journal.OpenBook(metadata).AcquireWindowWork(ctx, filepath.Join(s.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, id)
	if err != nil {
		return nil, err
	}
	if err = op.check(ctx); err != nil {
		return nil, err
	}
	return op, nil
}

func (p *standaloneReloadOperation) close() error {
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

func (p *standaloneReloadOperation) check(ctx context.Context) error {
	record, err := p.run.Check(ctx)
	if err != nil {
		return err
	}
	input, err := parseStandaloneReload(record)
	if err != nil {
		return err
	}
	if err := p.session.confirm(ctx, p.session.target); err != nil {
		return err
	}
	ready := p.session.ready
	if ready.Release != input.Expected.Release || ready.SessionNonce != input.Expected.SessionNonce || ready.Character != input.Expected.Character || ready.Realm != input.Expected.Realm || ready.Product != input.Expected.Product || ready.Build != input.Expected.Build || ready.GUID != input.GUID || ready.RuntimeEpoch+1 != input.Expected.RuntimeEpoch && ready.RuntimeEpoch != input.Expected.RuntimeEpoch {
		return errors.New("live.reload_session_mismatch")
	}
	return nil
}

func (p *standaloneReloadOperation) execute(ctx context.Context, send preparedInput) (journal.WorkRecord, error) {
	for step := 0; step < 3; step++ {
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return record, err
		}
		switch record.Stage {
		case "prepared":
			if err := p.submit(ctx, send); err != nil {
				return latestReloadRecord(ctx, p.metadata, p.id, err)
			}
		case "reload_requested":
			return p.observe(ctx)
		case "cleaned":
			return record, nil
		default:
			return record, journal.ErrTransition
		}
	}
	return journal.WorkRecord{}, errors.New("live.reload_step_limit")
}

func (p *standaloneReloadOperation) submit(ctx context.Context, send preparedInput) error {
	if send == nil {
		return errors.New("live.input_sender_missing")
	}
	prepared := false
	firstInput := false
	receipt, sendErr := send(ctx, p.session.target.Window, func(ctx context.Context) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
		if err != nil {
			return "", err
		}
		input, err := parseStandaloneReload(record)
		if err != nil {
			return "", err
		}
		observation, _ := json.Marshal(standaloneReloadObservation{Schema: "lycheedev.reload-observation.v1"})
		if err := journal.OpenBook(p.metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "prepared", Stage: "reload_requested", Status: "running", Observation: observation}); err != nil {
			return "", err
		}
		prepared, firstInput = true, true
		return "/dev bridge refresh " + input.ReloadNonce, nil
	}, func(ctx context.Context) error {
		if err := p.check(ctx); err != nil {
			return err
		}
		if firstInput {
			if err := p.session.reader.RequireFreshSignal(); err != nil {
				return err
			}
			firstInput = false
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
	var observed standaloneReloadObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return errors.Join(sendErr, err)
	}
	observed.Input = &receipt
	raw, _ := json.Marshal(observed)
	status := "running"
	if sendErr != nil {
		status = "unresolved"
	}
	err = journal.OpenBook(p.metadata).AdvanceStage(persist, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "reload_requested", Stage: "reload_requested", Status: status, Observation: raw})
	return errors.Join(sendErr, err)
}

func (p *standaloneReloadOperation) observe(ctx context.Context) (journal.WorkRecord, error) {
	if err := p.check(ctx); err != nil {
		return journal.WorkRecord{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return record, err
	}
	input, err := parseStandaloneReload(record)
	if err != nil {
		return record, err
	}
	wait, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	signal, err := p.session.reader.WaitForSignal(wait, input.Expected)
	if err != nil {
		return record, err
	}
	if signal.GUID != input.GUID || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CleanupNonce != "" {
		return record, errors.New("live.invalid_reload_receipt")
	}
	rawSignal, _ := json.Marshal(signal)
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return record, err
	}
	capture, err := evidence.OpenArchive(store, p.metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(rawSignal), MaxBytes: 4096, MediaType: "application/json", Complete: true,
		Provenance: evidence.Provenance{Kind: "decoded-standalone-reload", Locator: input.ReloadNonce, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
	if err != nil {
		return record, err
	}
	var observed standaloneReloadObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return record, err
	}
	observed.ReadyCapture = capture.ID
	raw, _ := json.Marshal(observed)
	book := journal.OpenBook(p.metadata)
	if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "reload_requested", Stage: "cleaned", Status: "completed", Observation: raw}); err != nil {
		return record, err
	}
	completed, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return completed, err
	}
	p.session.ready = signal
	if err := p.run.Close(); err != nil {
		return completed, err
	}
	p.run = nil
	if err := book.RetireWindowWork(ctx, filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns"), store.Identity().WorkspaceID, p.id); err != nil {
		return completed, err
	}
	return completed, nil
}

func latestReloadRecord(ctx context.Context, metadata *vault.Metadata, id string, cause error) (journal.WorkRecord, error) {
	read, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	record, err := journal.OpenBook(metadata).InspectWork(read, id)
	return record, errors.Join(cause, err)
}

// Reload has no probe report. Completion requires the archived correlated ready
// receipt, not merely a terminal journal stage. Status never sends game input.
func reloadOutcome(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, result Outcome) (Outcome, error) {
	input, err := parseStandaloneReload(record)
	if err != nil {
		return result, err
	}
	if record.Stage != "cleaned" || record.Status != "completed" {
		return result, nil
	}
	var observed standaloneReloadObservation
	if json.Unmarshal(record.Observation, &observed) != nil || observed.Schema != "lycheedev.reload-observation.v1" || observed.ReadyCapture == "" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	ref, data, err := archive.FetchCapture(ctx, observed.ReadyCapture, 4096)
	if err != nil {
		return result, err
	}
	want := evidence.Provenance{Kind: "decoded-standalone-reload", Locator: input.ReloadNonce, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	signal, err := bridge.ParseSignal(data)
	if err != nil {
		return result, err
	}
	if err := signal.Match(input.Expected); err != nil {
		return result, err
	}
	if signal.GUID != input.GUID || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CleanupNonce != "" {
		return result, errors.New("live.invalid_reload_receipt")
	}
	result.Complete = true
	result.RuntimeCapture = ref.ID
	return result, nil
}

func resumeStandaloneReload(ctx context.Context, root, id string, region image.Rectangle,
	capture func(context.Context, ClientWindow, image.Rectangle) (sessionFrames, error),
	confirm func(context.Context, ClientWindow) error, send preparedInput,
) (journal.WorkRecord, error) {
	record, err := InspectOperation(ctx, root, id)
	if err != nil {
		return record, err
	}
	if record.Stage == "cleaned" {
		return ReleaseCompletedReload(ctx, root, record)
	}
	input, err := parseStandaloneReload(record)
	if err != nil {
		return record, err
	}
	bound, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil {
		return record, err
	}
	if err := confirm(ctx, bound.Target); err != nil {
		return record, err
	}
	frames, err := capture(ctx, bound.Target, region)
	if err != nil {
		return record, err
	}
	ready := bound.Ready
	if record.Stage == "reload_requested" {
		ready.RuntimeEpoch = input.Expected.RuntimeEpoch
	}
	session := &WindowSession{target: bound.Target, region: region, ready: ready, reader: bridge.ObserveSignals(frames), frames: frames, confirm: confirm}
	defer session.Close()
	op, err := session.openStandaloneReload(ctx, root, id)
	if err != nil {
		return record, err
	}
	defer op.close()
	return op.execute(ctx, send)
}

// ReleaseCompletedReload closes the crash window between the durable terminal
// record and retirement of the shared window marker. It is safe to repeat.
func ReleaseCompletedReload(ctx context.Context, root string, record journal.WorkRecord) (journal.WorkRecord, error) {
	input, err := parseStandaloneReload(record)
	if err != nil || record.Stage != "cleaned" || record.Status != "completed" {
		return record, errors.Join(err, journal.ErrTransition)
	}
	_, err = vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (struct{}, error) {
		return struct{}{}, journal.OpenBook(metadata).RetireWindowWork(ctx, filepath.Join(input.Installation, "Interface", "AddOns"), store.Identity().WorkspaceID, record.OperationID)
	})
	return record, err
}
