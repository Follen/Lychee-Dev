package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"time"
)

func cleanupExpectation(input ReportIntent, binding RecordedSession, observed reportObservation) (bridge.SignalExpectation, error) {
	var zero bridge.SignalExpectation
	if observed.ReloadedCapture == "" || observed.CleanupReadyID == "" || observed.QueueRetirement == nil || observed.QueueRetirement.Revision.SHA256 == "" {
		return zero, errors.New("live.cleanup_prerequisite_missing")
	}
	if nonce, err := hex.DecodeString(observed.CleanupNonce); err != nil || len(nonce) != 16 || hex.EncodeToString(nonce) != observed.CleanupNonce {
		return zero, errors.New("live.invalid_cleanup_nonce")
	}
	if binding.Ready.RuntimeEpoch == 0 || binding.Ready.RuntimeEpoch > 9007199254740989 {
		return zero, errors.New("live.reload_epoch_unavailable")
	}
	expected := input.Expected
	expected.Kind, expected.CleanupNonce, expected.ReloadNonce = "cleared", observed.CleanupNonce, ""
	expected.RuntimeEpoch, expected.AfterSequence, expected.RequireInputReady = binding.Ready.RuntimeEpoch+2, 0, false
	return expected, nil
}

func validateCleanupReentry(signal bridge.Signal, input ReportIntent, binding RecordedSession, observed reportObservation) error {
	expected, err := cleanupExpectation(input, binding, observed)
	if err != nil {
		return err
	}
	if err := signal.Match(expected); err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID || signal.InputReady || signal.ReloadNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.Sequence > 9007199254740991 {
		return errors.New("live.invalid_cleanup_reentry")
	}
	return nil
}

func clearedProvenance(record journal.WorkRecord, input ReportIntent) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-game-cleared", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
}

func validateCleanupReadiness(signal bridge.Signal, input ReportIntent, epoch, after uint64) error {
	expected := input.Expected
	expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
	expected.RuntimeEpoch, expected.AfterSequence, expected.RequireInputReady = epoch, after, true
	if err := signal.Match(expected); err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CleanupNonce != "" || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.Sequence > 9007199254740991 {
		return errors.New("live.invalid_cleanup_readiness")
	}
	return nil
}

func readCleanupReadiness(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, input ReportIntent, binding RecordedSession, observed reportObservation) error {
	ref, raw, err := archive.FetchCapture(ctx, observed.CleanupReadyID, 4096)
	if err != nil {
		return err
	}
	want := clearedProvenance(record, input)
	want.Kind = "decoded-cleanup-readiness"
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return errors.New("live.cleanup_readiness_provenance_mismatch")
	}
	ready, err := bridge.ParseSignal(raw)
	if err != nil {
		return err
	}
	_, raw, err = archive.FetchCapture(ctx, observed.AcknowledgementID, 4096)
	if err != nil {
		return err
	}
	ack, err := bridge.ParseSignal(raw)
	if err != nil {
		return err
	}
	return validateCleanupReadiness(ready, input, binding.Ready.RuntimeEpoch+1, ack.Sequence)
}

// A committed handoff remains independently verifiable after interruption;
// neither the stage nor the in-memory session epoch is used as its evidence.
func readCleanupEvidence(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession, observed reportObservation) (bridge.Signal, evidence.CaptureRef, error) {
	type result struct {
		signal  bridge.Signal
		capture evidence.CaptureRef
	}
	value, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (result, error) {
		var zero result
		if record.Stage != "acknowledged" && record.Stage != "cleaned" {
			return zero, journal.ErrTransition
		}
		base, err := executionBase(ctx, root, record, input, binding)
		if err != nil {
			return zero, err
		}
		if _, err := cleanupExpectation(input, base, observed); err != nil {
			return zero, err
		}
		if _, _, err := readReloadEvidence(ctx, root, record, input, binding, observed.ReloadedCapture); err != nil {
			return zero, err
		}
		archive := evidence.OpenArchive(store, metadata)
		report, err := acknowledgedReport(ctx, archive, record, input, observed)
		if err != nil {
			return zero, err
		}
		if err := readCleanupReadiness(ctx, archive, record, input, base, observed); err != nil {
			return zero, err
		}
		if record.Stage == "cleaned" {
			ref, raw, err := archive.FetchCapture(ctx, observed.RemovalID, bridge.SavedStateLimits().FileBytes)
			if err != nil {
				return zero, err
			}
			want := clearedProvenance(record, input)
			want.Kind, want.Locator = "game-report-removal", observed.SourcePath
			if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "text/plain" || ref.Blob.SHA256 != observed.RemovalSHA256 || observed.RemovalSHA256 == observed.SourceSHA256 {
				return zero, errors.New("live.removal_provenance_mismatch")
			}
			if _, err := bridge.VerifyArchivedReportRemoval(report.ReceiptBytes, report.Body, bytes.NewReader(raw), input.Code, input.Expected); err != nil {
				return zero, err
			}
		}
		ref, raw, err := archive.FetchCapture(ctx, observed.ClearedID, 4096)
		if err != nil {
			return zero, err
		}
		if ref.Provenance != clearedProvenance(record, input) || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return zero, errors.New("live.cleanup_provenance_mismatch")
		}
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			return zero, err
		}
		if err := validateCleanupReentry(signal, input, base, observed); err != nil {
			return zero, err
		}
		return result{signal, ref}, nil
	})
	return value.signal, value.capture, err
}

// Explicitly crosses one known runtime transition, retaining the same native
// window lease and stream. Ordinary p.check must reject a stale adopted epoch.
func (p *ProbeOperation) cleanupReentryContext(ctx context.Context) (journal.WorkRecord, ReportIntent, RecordedSession, reportObservation, error) {
	var record journal.WorkRecord
	var input ReportIntent
	var binding RecordedSession
	var observed reportObservation
	if p == nil || p.run == nil || p.metadata == nil {
		return record, input, binding, observed, errors.New("live.operation_closed")
	}
	record, err := p.run.Check(ctx)
	if err != nil {
		return record, input, binding, observed, err
	}
	if record.Stage != "acknowledged" || (record.Status != "running" && record.Status != "unresolved") {
		return record, input, binding, observed, journal.ErrTransition
	}
	input, binding, err = p.session.boundOperationInput(ctx, p.root, record)
	if err != nil {
		return record, input, binding, observed, err
	}
	if err = json.Unmarshal(record.Observation, &observed); err != nil {
		return record, input, binding, observed, err
	}
	base, err := executionBase(ctx, p.root, record, input, binding)
	if err != nil {
		return record, input, binding, observed, err
	}
	expected, err := cleanupExpectation(input, base, observed)
	if err != nil {
		return record, input, binding, observed, err
	}
	if p.session.ready.RuntimeEpoch != expected.RuntimeEpoch-1 && p.session.ready.RuntimeEpoch != expected.RuntimeEpoch {
		return record, input, binding, observed, errors.New("live.operation_runtime_mismatch")
	}
	if _, _, err = readReloadEvidence(ctx, p.root, record, input, binding, observed.ReloadedCapture); err != nil {
		return record, input, binding, observed, err
	}
	if err = p.session.confirm(ctx, p.session.target); err != nil {
		return record, input, binding, observed, err
	}
	_, err = p.run.Check(ctx)
	return record, input, binding, observed, err
}

func (p *ProbeOperation) observeCleanupReentry(ctx context.Context) (bridge.Signal, error) {
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (bridge.Signal, error) {
		var zero bridge.Signal
		record, input, binding, observed, err := p.cleanupReentryContext(ctx)
		if err != nil {
			return zero, err
		}
		base, err := executionBase(ctx, p.root, record, input, binding)
		if err != nil {
			return zero, err
		}
		archive := evidence.OpenArchive(store, metadata)
		if _, err := acknowledgedReport(ctx, archive, record, input, observed); err != nil {
			return zero, err
		}
		if err := readCleanupReadiness(ctx, archive, record, input, base, observed); err != nil {
			return zero, err
		}
		_, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		queuePath := filepath.Join(p.session.target.Client.Directory, "Interface", "AddOns", "Lychee Dev")
		if _, err := delivery.VerifyProbeRetired(ctx, queuePath, definition); err != nil {
			return zero, err
		}
		var existing evidence.CaptureRef
		if observed.ClearedID != "" {
			_, existing, err = readCleanupEvidence(ctx, p.root, record, input, binding, observed)
			if err != nil {
				return zero, err
			}
		}
		expected, err := cleanupExpectation(input, base, observed)
		if err != nil {
			return zero, err
		}
		wait, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		signal, err := p.session.reader.WaitForSignal(wait, expected)
		if err != nil {
			return zero, err
		}
		if err := validateCleanupReentry(signal, input, base, observed); err != nil {
			return zero, err
		}
		if _, _, _, _, err := p.cleanupReentryContext(ctx); err != nil {
			return zero, err
		}
		if _, err := delivery.VerifyProbeRetired(ctx, queuePath, definition); err != nil {
			return zero, err
		}
		if existing.ID == "" {
			raw, _ := json.Marshal(signal)
			capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: clearedProvenance(record, input)})
			if err != nil {
				return zero, err
			}
			observed.ClearedID = capture.ID
			raw, _ = json.Marshal(observed)
			if err := journal.OpenBook(metadata).AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: "acknowledged", Stage: "acknowledged", Status: "running", Observation: raw}); err != nil {
				return zero, err
			}
		}
		p.session.ready = signal
		return signal, nil
	})
}
