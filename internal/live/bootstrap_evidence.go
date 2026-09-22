package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

type bootstrapObservation struct {
	// Committed before sending input. Presence means reconcile, never resend;
	// BootstrapInput is diagnostic and may be absent after process interruption.
	BootstrapReadyID string                `json:"bootstrapReadyId,omitempty"`
	BootstrapCapture string                `json:"bootstrapCapture,omitempty"`
	BootstrapInput   *desktop.InputReceipt `json:"bootstrapInput,omitempty"`
}

func bootstrapExpectation(input ReportIntent, binding RecordedSession) (bridge.SignalExpectation, error) {
	if binding.Ready.RuntimeEpoch == 0 || binding.Ready.RuntimeEpoch >= 9007199254740991 {
		return bridge.SignalExpectation{}, errors.New("live.reload_epoch_unavailable")
	}
	expected := input.Expected
	expected.Kind, expected.ReloadNonce = "ready", input.Load.ReloadNonce
	expected.RuntimeEpoch, expected.AfterSequence, expected.RequireInputReady = binding.Ready.RuntimeEpoch+1, 0, true
	return expected, nil
}

func bootstrapProvenance(record journal.WorkRecord, input ReportIntent) evidence.Provenance {
	return evidence.Provenance{Kind: "decoded-queue-reentry", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
}

func validateBootstrap(signal bridge.Signal, input ReportIntent, binding RecordedSession) error {
	expected, err := bootstrapExpectation(input, binding)
	if err != nil {
		return err
	}
	if err := signal.Match(expected); err != nil {
		return err
	}
	if signal.GUID != input.Load.GUID || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.CleanupNonce != "" {
		return errors.New("live.invalid_bootstrap_signal")
	}
	return nil
}

func readBootstrapEvidence(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession, observed bootstrapObservation) (bridge.Signal, evidence.CaptureRef, error) {
	type result struct {
		signal  bridge.Signal
		capture evidence.CaptureRef
	}
	value, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (result, error) {
		var zero result
		if observed.BootstrapReadyID == "" || record.Stage == "prepared" {
			return zero, errors.New("live.bootstrap_intent_missing")
		}
		archive := evidence.OpenArchive(store, metadata)
		readyRef, raw, err := archive.FetchCapture(ctx, observed.BootstrapReadyID, 4096)
		if err != nil {
			return zero, err
		}
		want := bootstrapProvenance(record, input)
		want.Kind = "decoded-bootstrap-readiness"
		if readyRef.Provenance != want || !readyRef.Complete || readyRef.Truncated || readyRef.MediaType != "application/json" {
			return zero, errors.New("live.bootstrap_readiness_provenance_mismatch")
		}
		ready, err := bridge.ParseSignal(raw)
		if err != nil {
			return zero, err
		}
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.AfterSequence, expected.RuntimeEpoch, expected.RequireInputReady = binding.Ready.Sequence-1, binding.Ready.RuntimeEpoch, true
		if err := ready.Match(expected); err != nil {
			return zero, err
		}
		if ready.RequestID != "" || ready.ReloadNonce != "" || ready.GUID != input.Load.GUID || ready.CodeBytes != 0 || ready.CodeAdler32 != "" || ready.ReportBytes != 0 || ready.ReportAdler32 != "" {
			return zero, errors.New("live.invalid_bootstrap_readiness")
		}
		ref, raw, err := archive.FetchCapture(ctx, observed.BootstrapCapture, 4096)
		if err != nil {
			return zero, err
		}
		if ref.Provenance != bootstrapProvenance(record, input) || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return zero, errors.New("live.bootstrap_evidence_mismatch")
		}
		signal, err := bridge.ParseSignal(raw)
		if err != nil {
			return zero, err
		}
		if err := validateBootstrap(signal, input, binding); err != nil {
			return zero, err
		}
		return result{signal, ref}, nil
	})
	return value.signal, value.capture, err
}

// Derive an execution anchor without changing the frozen original binding.
// Callers pass the original binding, never a previously derived value.
func executionBase(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession) (RecordedSession, error) {
	var observed bootstrapObservation
	if len(record.Observation) != 0 {
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return binding, err
		}
	}
	if observed.BootstrapCapture != "" {
		ready, _, err := readBootstrapEvidence(ctx, root, record, input, binding, observed)
		if err != nil {
			return binding, err
		}
		binding.Ready = ready
	}
	return binding, nil
}
