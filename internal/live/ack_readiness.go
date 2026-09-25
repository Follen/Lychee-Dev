package live

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// ACK readiness is a historical input anchor, not proof that the requested
// flush caused persistence. Every new input still needs fresh window frames.
func readAckReadiness(ctx context.Context, root string, record journal.WorkRecord, input ReportIntent, binding RecordedSession, anchor bridge.Signal, id string, reloaded bool) (bridge.Signal, error) {
	return vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (bridge.Signal, error) {
		var zero bridge.Signal
		switch record.Stage {
		case "verified", "ack_requested", "acknowledged", "cleaned":
		default:
			return zero, journal.ErrTransition
		}
		ref, data, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, id, 4096)
		if err != nil {
			return zero, err
		}
		want := evidence.Provenance{Kind: "decoded-ack-readiness", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
		if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return zero, errors.New("live.ack_readiness_evidence_mismatch")
		}
		signal, err := bridge.ParseSignal(data)
		if err != nil {
			return zero, err
		}
		if !reloaded {
			if input.Revision == "" {
				return zero, errors.New("live.reload_not_observed")
			}
			base, err := executionBase(ctx, root, record, input, binding)
			if err != nil {
				return zero, err
			}
			return signal, validateReloadSignal(signal, input, base)
		}
		expected := input.Expected
		expected.Kind, expected.RequestID, expected.ReloadNonce = "ready", "", ""
		expected.RuntimeEpoch, expected.RequireInputReady = anchor.RuntimeEpoch, true
		expected.AfterSequence = anchor.Sequence - 1
		if err := signal.Match(expected); err != nil {
			return zero, err
		}
		generic := signal.RequestID == "" && signal.ReloadNonce == ""
		correlated := signal.RequestID == input.Expected.RequestID && signal.ReloadNonce == input.Load.ReloadNonce
		if (!generic && !correlated) || signal.GUID != input.Load.GUID || signal.CodeBytes != 0 || signal.CodeAdler32 != "" || signal.ReportBytes != 0 || signal.ReportAdler32 != "" || signal.CleanupNonce != "" {
			return zero, errors.New("live.invalid_ack_readiness")
		}
		return signal, nil
	})
}
