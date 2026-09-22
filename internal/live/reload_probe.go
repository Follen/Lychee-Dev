package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"time"
)

func reloadExpectation(input ReportIntent, binding RecordedSession) (bridge.SignalExpectation, error) {
	if binding.Ready.RuntimeEpoch == 0 || binding.Ready.RuntimeEpoch >= 9007199254740991 {
		return bridge.SignalExpectation{}, errors.New("live.reload_epoch_unavailable")
	}
	expected := input.Expected
	expected.Kind, expected.ReloadNonce = "ready", input.Load.ReloadNonce
	expected.RuntimeEpoch = binding.Ready.RuntimeEpoch + 1
	expected.AfterSequence, expected.RequireInputReady = 0, true
	return expected, nil
}

func validateReloadSignal(signal bridge.Signal, input ReportIntent, binding RecordedSession) error {
	expected, err := reloadExpectation(input, binding)
	if err != nil {
		return err
	}
	if err := signal.Match(expected); err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.CleanupNonce != "" || signal.Sequence > 9007199254740991 {
		return errors.New("live.invalid_reload_signal")
	}
	return nil
}

func reloadProvenance(record journal.WorkRecord, input ReportIntent) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-game-reentry", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
}

func readReloadEvidence(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession, id string) (bridge.Signal, evidence.CaptureRef, error) {
	type result struct {
		Signal  bridge.Signal
		Capture evidence.CaptureRef
	}
	value, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (result, error) {
		var zero result
		base, err := executionBase(ctx, root, record, input, binding)
		if err != nil {
			return zero, err
		}
		switch record.Stage {
		case "flush_requested", "persisted", "verified", "ack_requested", "acknowledged", "cleaned":
		default:
			return zero, journal.ErrTransition
		}
		ref, data, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, id, 4096)
		if err != nil {
			return zero, err
		}
		if ref.Provenance != reloadProvenance(record, input) || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return zero, errors.New("live.reload_evidence_mismatch")
		}
		signal, err := bridge.ParseSignal(data)
		if err != nil {
			return zero, err
		}
		if err := validateReloadSignal(signal, input, base); err != nil {
			return zero, err
		}
		return result{signal, ref}, nil
	})
	return value.Signal, value.Capture, err
}

// reloadContext deliberately validates the immutable binding and the native
// window lease, not a stale Lua epoch. Only ObserveReload uses this path, and it
// must prove the exact next epoch from fresh frames before changing the session.
func (p *ProbeOperation) reloadContext(ctx context.Context) (journal.WorkRecord, ReportIntent, RecordedSession, error) {
	var record journal.WorkRecord
	var input ReportIntent
	var binding RecordedSession
	if p == nil || p.run == nil || p.metadata == nil {
		return record, input, binding, errors.New("live.operation_closed")
	}
	record, err := p.run.Check(ctx)
	if err != nil {
		return record, input, binding, err
	}
	if record.Stage != "flush_requested" || (record.Status != "running" && record.Status != "unresolved") {
		return record, input, binding, journal.ErrTransition
	}
	input, binding, err = p.session.boundOperationInput(ctx, p.root, record)
	if err != nil {
		return record, input, binding, err
	}
	base, err := executionBase(ctx, p.root, record, input, binding)
	if err != nil {
		return record, input, binding, err
	}
	if p.session.ready.RuntimeEpoch != base.Ready.RuntimeEpoch && p.session.ready.RuntimeEpoch != base.Ready.RuntimeEpoch+1 {
		return record, input, binding, errors.New("live.operation_runtime_mismatch")
	}
	if err := p.session.confirm(ctx, p.session.target); err != nil {
		return record, input, binding, err
	}
	_, err = p.run.Check(ctx)
	return record, input, binding, err
}

// ObserveReload confirms a one-use addon reentry ticket, not persisted report
// bytes. It keeps the same capture stream and OS lease, never sends input, and
// can reconcile a committed transition whose in-memory update was interrupted.
func (p *ProbeOperation) ObserveReload(ctx context.Context) (evidence.CaptureRef, error) {
	if _, _, _, err := p.reloadContext(ctx); err != nil {
		return evidence.CaptureRef{}, err
	}
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		record, input, binding, err := p.reloadContext(ctx)
		if err != nil {
			return zero, err
		}
		base, err := executionBase(ctx, p.root, record, input, binding)
		if err != nil {
			return zero, err
		}
		expected, err := reloadExpectation(input, base)
		if err != nil {
			return zero, err
		}
		_, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		archive := evidence.OpenArchive(store, metadata)
		if _, err := reportedOperationEvidence(ctx, archive, record, input, definition); err != nil {
			return zero, err
		}
		var observed probeLoadObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return zero, err
		}
		var existing evidence.CaptureRef
		if observed.ReloadedCapture != "" {
			_, existing, err = readReloadEvidence(ctx, p.root, record, input, binding, observed.ReloadedCapture)
			if err != nil {
				return zero, err
			}
		}
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return zero, err
		}
		if err := validateReloadSignal(signal, input, base); err != nil {
			return zero, err
		}
		if _, _, _, err := p.reloadContext(ctx); err != nil {
			return zero, err
		}
		if existing.ID != "" {
			p.session.ready = signal
			return existing, nil
		}
		raw, _ := json.Marshal(signal)
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: reloadProvenance(record, input)})
		if err != nil {
			return zero, err
		}
		observed.ReloadedCapture = capture.ID
		raw, _ = json.Marshal(observed)
		if _, _, _, err := p.reloadContext(ctx); err != nil {
			return zero, err
		}
		if err := journal.OpenBook(metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "flush_requested", Stage: "flush_requested", Status: "running", Observation: raw}); err != nil {
			return zero, err
		}
		p.session.ready = signal
		return capture, nil
	})
}
