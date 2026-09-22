package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Both retirement and disk-removal verification cross this same evidence seam.
// A recorded stage alone never proves that the game acknowledged the report.
func acknowledgedReport(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, input ReportIntent, observed reportObservation) (bridge.VerifiedReport, error) {
	var zero bridge.VerifiedReport
	if observed.Schema != "lycheedev.report-observation.v1" || observed.SourcePath == "" || observed.SourceSHA256 == "" || observed.AcknowledgementID == "" {
		return zero, errors.New("live.removal_evidence_missing")
	}
	report, err := archive.ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
	if err != nil {
		return zero, err
	}
	ref, raw, err := archive.FetchCapture(ctx, observed.AcknowledgementID, 4096)
	if err != nil {
		return zero, err
	}
	want := evidence.Provenance{Kind: "decoded-game-acknowledgement", Locator: input.Expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return zero, errors.New("live.acknowledgement_provenance_mismatch")
	}
	ack, err := bridge.ParseSignal(raw)
	if err != nil {
		return zero, err
	}
	expected := input.Expected
	expected.Kind, expected.AfterSequence, expected.RequireInputReady = "acknowledged", report.Receipt.Sequence, false
	if err := ack.Match(expected); err != nil {
		return zero, err
	}
	if ack.CodeBytes != report.Receipt.CodeBytes || ack.CodeAdler32 != report.Receipt.CodeAdler32 || ack.ReportBytes != report.Receipt.ReportBytes || ack.ReportAdler32 != report.Receipt.ReportAdler32 {
		return zero, errors.New("live.acknowledgement_payload_mismatch")
	}
	return report, nil
}
