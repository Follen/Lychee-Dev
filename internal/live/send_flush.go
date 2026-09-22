package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"time"
)

// Flush queues one reload only after a new, input-ready session receipt newer
// than the observed report. It cannot manufacture that receipt or prove disk
// persistence. The supplied readiness must already be displayed in the game.
func (p *ProbeOperation) Flush(ctx context.Context) (desktop.InputReceipt, error) {
	return p.flush(ctx, desktop.QueuePreparedCommand)
}

func (p *ProbeOperation) flush(ctx context.Context, send preparedInput) (desktop.InputReceipt, error) {
	if err := p.check(ctx); err != nil {
		return desktop.InputReceipt{}, err
	}
	record, err := journal.OpenBook(p.metadata).InspectWork(ctx, p.id)
	if err != nil {
		return desktop.InputReceipt{}, err
	}
	if record.Stage != "reported" || record.Status != "running" {
		return desktop.InputReceipt{}, journal.ErrTransition
	}
	return p.submitInput(ctx, "flush_requested", send, func(ctx context.Context) (string, error) {
		if err := p.prepareFlushReadiness(ctx); err != nil {
			return "", err
		}
		return RequestOperationFlush(ctx, p.root, p.id)
	})
}

func (p *ProbeOperation) prepareFlushReadiness(ctx context.Context) error {
	if err := p.check(ctx); err != nil {
		return err
	}
	if p.session.ready.RuntimeEpoch == 0 || p.session.ready.RuntimeEpoch >= 9007199254740991 {
		return errors.New("live.reload_epoch_unavailable")
	}
	book := journal.OpenBook(p.metadata)
	record, err := book.InspectWork(ctx, p.id)
	if err != nil {
		return err
	}
	if record.Stage != "reported" || record.Status != "running" {
		return journal.ErrTransition
	}
	input, definition, err := probeDefinition(record)
	if err != nil {
		return err
	}
	store, err := vault.OpenStore(p.root)
	if err != nil {
		return err
	}
	archive := evidence.OpenArchive(store, p.metadata)
	reported, err := reportedOperationEvidence(ctx, archive, record, input, definition)
	if err != nil {
		return err
	}
	expected := input.Expected
	expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
	expected.AfterSequence, expected.RequireInputReady = reported.Sequence, true
	wait, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	signal, err := p.session.waitOperationSignal(wait, input.Load.Installation, expected)
	if err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.Sequence > 9007199254740991 {
		return errors.New("live.invalid_flush_readiness")
	}
	if err := p.check(ctx); err != nil {
		return err
	}
	raw, _ := json.Marshal(signal)
	capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-flush-readiness", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: p.id, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}})
	if err != nil {
		return err
	}
	var observed probeLoadObservation
	if err := json.Unmarshal(record.Observation, &observed); err != nil {
		return err
	}
	observed.FlushReadyCapture = capture.ID
	raw, _ = json.Marshal(observed)
	if err := p.check(ctx); err != nil {
		return err
	}
	return book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "reported", Stage: "reported", Status: "running", Observation: raw})
}
