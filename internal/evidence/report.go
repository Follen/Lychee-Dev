package evidence

import (
	"bytes"
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// ReportCaptures remains useful after interruption; no game action is performed.
// A partial return retains successfully committed evidence but cannot authorize ACK.
type ReportCaptures struct {
	Body    CaptureRef `json:"body"`
	Receipt CaptureRef `json:"receipt"`
}

func (a *Archive) CommitReport(ctx context.Context, receipt, body, code []byte, expected bridge.SignalExpectation, operationID, snapshot string) (ReportCaptures, error) {
	var captures ReportCaptures
	if operationID == "" || snapshot == "" {
		return captures, errors.New("evidence.report_context_required")
	}
	report, err := bridge.VerifyReport(receipt, body, code, expected)
	if err != nil {
		return captures, err
	}
	provenance := Provenance{Kind: "game-report", Locator: expected.RequestID, Snapshot: snapshot, OperationID: operationID, DataBuild: expected.Build, Session: expected.SessionNonce}
	captures.Body, err = a.CommitCapture(ctx, CaptureDraft{Reader: bytes.NewReader(report.Body), MaxBytes: 512 << 10, MediaType: "application/json", Provenance: provenance, Complete: true})
	if err != nil {
		return captures, err
	}
	provenance.Kind = "game-receipt"
	captures.Receipt, err = a.CommitCapture(ctx, CaptureDraft{Reader: bytes.NewReader(report.ReceiptBytes), MaxBytes: 4096, MediaType: "application/json", Provenance: provenance, Complete: true})
	return captures, err
}

// ReadVerifiedReport rereads both durable captures and verifies their
// original bytes and operation provenance. It is a read-only result retrieval;
// callers that acknowledge the report must separately persist their input intent.
func (a *Archive) ReadVerifiedReport(ctx context.Context, bodyID, receiptID string, code []byte, expected bridge.SignalExpectation, operationID, snapshot string) (bridge.VerifiedReport, error) {
	var zero bridge.VerifiedReport
	if operationID == "" || snapshot == "" {
		return zero, errors.New("evidence.report_context_required")
	}
	bodyRef, body, err := a.FetchCapture(ctx, bodyID, 512<<10)
	if err != nil {
		return zero, err
	}
	receiptRef, receipt, err := a.FetchCapture(ctx, receiptID, 4096)
	if err != nil {
		return zero, err
	}
	for index, ref := range []CaptureRef{bodyRef, receiptRef} {
		kind := "game-report"
		if index == 1 {
			kind = "game-receipt"
		}
		want := Provenance{Kind: kind, Locator: expected.RequestID, Snapshot: snapshot, OperationID: operationID, DataBuild: expected.Build, Session: expected.SessionNonce}
		if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
			return zero, errors.New("evidence.report_provenance_mismatch")
		}
	}
	return bridge.VerifyReport(receipt, body, code, expected)
}
