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
	"image/png"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrInvestigationPending = errors.New("live.investigation_pending")

// checkpoint wakes the dedicated receiver under the original owner. It never
// needs the old business QR, nor grants another execution or reload.
func (p *ProbeOperation) checkpoint(ctx context.Context, action string, send preparedInput) (bridge.Signal, evidence.CaptureRef, error) {
	var zero bridge.Signal
	var noRef evidence.CaptureRef
	if action != "observe" && action != "finish" {
		return zero, noRef, errors.New("live.invalid_checkpoint_action")
	}
	if err := p.check(ctx); err != nil {
		return zero, noRef, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return zero, noRef, err
	}
	if record.Intent.Goal != "finished" {
		return zero, noRef, journal.ErrTransition
	}
	input, err := reportInput(record)
	if err != nil {
		return zero, noRef, err
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return zero, noRef, err
	}
	challenge := hex.EncodeToString(nonce[:])
	p.session.reader.SetObservationFence(time.Now())
	_, sendErr := send(ctx, p.session.target.Window, func(ctx context.Context) (string, error) {
		if err := p.check(ctx); err != nil {
			return "", err
		}
		return "/dev bridge " + action + " " + input.Expected.RequestID + " " + challenge, nil
	}, p.check)
	if ctx.Err() != nil {
		return zero, noRef, errors.Join(sendErr, ctx.Err())
	}
	expected := input.Expected
	expected.Kind, expected.ProbeNonce, expected.ReloadNonce, expected.CleanupNonce = "checkpoint", challenge, "", ""
	expected.AfterSequence, expected.RuntimeEpoch, expected.RequireInputReady = 0, 0, false
	wait, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	signal, err := p.session.reader.WaitForSignal(wait, expected)
	if err != nil {
		return signal, noRef, errors.Join(sendErr, err)
	}
	if err := validateCheckpoint(signal, input, expected); err != nil {
		return signal, noRef, err
	}
	maximumEpoch := p.session.ready.RuntimeEpoch
	switch record.Stage {
	case "flush_requested", "persisted", "verified", "ack_requested", "acknowledged":
		maximumEpoch++ // The authorized report flush may have replaced its QR.
	}
	if signal.RuntimeEpoch < p.session.ready.RuntimeEpoch || signal.RuntimeEpoch > maximumEpoch || signal.RuntimeEpoch == p.session.ready.RuntimeEpoch && signal.Sequence <= p.session.ready.Sequence {
		return signal, noRef, errors.New("live.checkpoint_runtime_mismatch")
	}
	if err := p.check(ctx); err != nil {
		return signal, noRef, err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return signal, noRef, err
	}
	raw, _ := json.Marshal(signal)
	ref, err := evidence.OpenArchive(store, p.metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096,
		MediaType: "application/json", Complete: true, Provenance: checkpointProvenance(record, input, challenge)})
	if err == nil {
		link, _ := json.Marshal(checkpointAnchor{Capture: ref.ID, Challenge: challenge})
		previous, readErr := p.metadata.ReadDocument(ctx, "checkpoint/"+p.id)
		if readErr != nil && !errors.Is(readErr, vault.ErrMissingRecord) {
			return signal, ref, readErr
		}
		err = p.metadata.CommitDocuments(ctx, vault.Mutation{Key: "checkpoint/" + p.id, ExpectedGeneration: previous.Generation, Value: link})
		if err == nil {
			p.session.ready = checkpointReady(signal)
			p.session.reader.SetIdentityBaseline(sessionSignalIdentity(p.session.ready))
		}
	}
	return signal, ref, err
}

type checkpointAnchor struct {
	Capture   string `json:"capture"`
	Challenge string `json:"challenge"`
}

func checkpointReady(s bridge.Signal) bridge.Signal {
	return bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: s.Release,
		Product: s.Product, Build: s.Build, SessionNonce: s.SessionNonce, Character: s.Character,
		Realm: s.Realm, GUID: s.GUID, RuntimeEpoch: s.RuntimeEpoch, Sequence: s.Sequence, ReportScope: "character-v1"}
}

// This is an archived identity anchor, never fresh input readiness. The live
// command still has to observe the separate ready signal after the checkpoint.
func readCheckpointAnchor(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent) (bridge.Signal, bool, error) {
	var found bool
	result, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (bridge.Signal, error) {
		var zero bridge.Signal
		doc, err := metadata.ReadDocument(ctx, "checkpoint/"+record.OperationID)
		if errors.Is(err, vault.ErrMissingRecord) {
			return zero, nil
		}
		if err != nil {
			return zero, err
		}
		var link checkpointAnchor
		if json.Unmarshal(doc.Value, &link) != nil || link.Capture == "" || len(link.Challenge) != 32 {
			return zero, errors.New("live.invalid_checkpoint_anchor")
		}
		ref, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, link.Capture, 4096)
		if err != nil {
			return zero, err
		}
		if !ref.Complete || ref.Truncated || ref.MediaType != "application/json" || ref.Provenance != checkpointProvenance(record, input, link.Challenge) {
			return zero, errors.New("live.checkpoint_provenance_mismatch")
		}
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			return zero, err
		}
		expected := input.Expected
		expected.Kind = "checkpoint"
		expected.ProbeNonce = link.Challenge
		expected.AfterSequence = 0
		expected.RuntimeEpoch = 0
		expected.RequireInputReady = false
		if err := validateCheckpoint(signal, input, expected); err != nil {
			return zero, err
		}
		found = true
		return checkpointReady(signal), nil
	})
	return result, found, err
}

func checkpointProvenance(record journal.WorkRecord, input ReportIntent, challenge string) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-game-checkpoint", Locator: challenge, OperationID: record.OperationID,
		Snapshot: record.Intent.Snapshot, Session: input.Expected.SessionNonce, DataBuild: input.Expected.Build}
}

func validateCheckpoint(signal bridge.Signal, input ReportIntent, expected bridge.SignalExpectation) error {
	if err := signal.Match(expected); err != nil {
		return err
	}
	if input.Load == nil || signal.GUID != input.Load.GUID || signal.CodeSHA256 != fmt.Sprintf("%x", sha256.Sum256(input.Code)) {
		return errors.New("live.checkpoint_identity_mismatch")
	}
	return nil
}

func (p *ProbeOperation) observeCheckpoint(ctx context.Context, send preparedInput) (evidence.CaptureRef, error) {
	signal, ref, err := p.checkpoint(ctx, "observe", send)
	if err != nil {
		return ref, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return ref, err
	}
	if signal.WorkState == "loaded" || signal.WorkState == "running" {
		return ref, ErrInvestigationPending
	}
	receipt, err := bridge.ParseSignal([]byte(signal.Receipt))
	if err != nil {
		return ref, err
	}
	receipt = bridge.FillSignalIdentity(receipt, sessionSignalIdentity(p.session.ready))
	wait := func(ctx context.Context, expected bridge.SignalExpectation) (bridge.Signal, error) {
		if err := p.check(ctx); err != nil {
			return receipt, err
		}
		if err := receipt.Match(expected); err != nil {
			return receipt, err
		}
		return receipt, nil
	}
	switch record.Stage {
	case "dispatch_requested":
		if receipt.Kind != "reported" && receipt.Kind != "report_error" {
			return ref, ErrInvestigationPending
		}
		return observeOperationReported(ctx, p.root, p.id, wait)
	case "ack_requested":
		if receipt.Kind != "acknowledged" {
			return ref, ErrInvestigationPending
		}
		return observeReportAcknowledgement(ctx, p.root, p.id, wait)
	}
	return ref, journal.ErrTransition
}

// finalizeDisplay retains the same driver/owner until a correlated terminal
// proof AND later valid WGC frames establish that its transient card disappeared.
func (p *ProbeOperation) finalizeDisplay(ctx context.Context, send preparedInput) error {
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	input, err := reportInput(record)
	if err != nil {
		return err
	}
	if _, found, err := readDisplayCompletion(ctx, p.root, record, input.Binding); err != nil || found {
		return err
	}
	signal, checkpoint, err := p.checkpoint(ctx, "finish", send)
	if err != nil {
		return err
	}
	if signal.WorkState != "finished" || !signal.ResourcesReleased {
		return ErrInvestigationPending
	}
	// Retained positive proof alone does not claim optical cleanup. Read only
	// newer frames on this same WGC feed; no rectangle/desktop fallback.
	after := time.Now()
	read, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	var last *desktop.CapturedFrame
	var ticks int64
	consecutive := 0
	for consecutive < 2 {
		frame, err := p.session.frames.Next(read)
		if err != nil {
			return errors.Join(ErrReceiptHidePending, err)
		}
		if frame == nil || frame.NRGBA == nil || !frame.ObservedAt.After(after) || frame.SystemTicks <= ticks {
			continue
		}
		ticks = frame.SystemTicks
		symbols, err := desktop.DecodeSymbols(frame.NRGBA)
		if err != nil || len(symbols) != 0 || frameHint(frame) != captureFrame {
			consecutive = 0
			continue
		}
		last = frame
		consecutive++
	}
	if err := p.check(ctx); err != nil {
		return err
	}
	var pixels bytes.Buffer
	if err := png.Encode(&pixels, last.NRGBA); err != nil {
		return err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return err
	}
	archive := evidence.OpenArchive(store, p.metadata)
	frame, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: &pixels, MaxBytes: 32 << 20, MediaType: "image/png", Complete: true,
		Provenance: evidence.Provenance{Kind: "wgc-finished-display", Locator: checkpoint.ID, OperationID: p.id, Snapshot: record.Intent.Snapshot, Session: record.Intent.Session}})
	if err != nil {
		return err
	}
	proof := displayCompletion{Schema: "lycheedev.display-completion.v1", Operation: p.id, Binding: input.Binding,
		Receipt:    HideReceiptResult{Session: input.Binding, Cleared: true, ObservedAt: last.ObservedAt},
		Checkpoint: checkpoint.ID, Challenge: signal.ProbeNonce, Frame: frame.ID, RuntimeEpoch: signal.RuntimeEpoch}
	_, err = saveDisplayProof(ctx, p.root, record, proof)
	return err
}
