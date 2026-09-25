package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
)

// ReportIntent is frozen in WorkIntent.Request before dispatch. Identity comes
// from the selected live session, never from the report being verified.
type ReportIntent struct {
	Schema   string                   `json:"schema"`
	Revision string                   `json:"revision,omitempty"`
	Expected bridge.SignalExpectation `json:"expected"`
	Code     []byte                   `json:"code"`
	Load     *ProbeLoadIntent         `json:"load,omitempty"`
	Binding  string                   `json:"binding,omitempty"`
}

type reportObservation struct {
	bootstrapObservation
	Schema            string           `json:"schema"`
	SourcePath        string           `json:"sourcePath,omitempty"`
	SourceSHA256      string           `json:"sourceSHA256,omitempty"`
	BodyID            string           `json:"bodyId"`
	ReceiptID         string           `json:"receiptId"`
	AcknowledgementID string           `json:"acknowledgementId,omitempty"`
	RemovalID         string           `json:"removalId,omitempty"`
	RemovalSHA256     string           `json:"removalSHA256,omitempty"`
	QueueRetirement   *queueRetirement `json:"queueRetirement,omitempty"`
	CleanupNonce      string           `json:"cleanupNonce,omitempty"`
	ClearedID         string           `json:"clearedId,omitempty"`
	ReloadedCapture   string           `json:"reloadedCapture,omitempty"`
	// Submission is diagnostic, not execution evidence. Preserve the earlier
	// transport chain through verification and ACK; old records may omit it.
	LoadedCapture     string                `json:"loadedCapture,omitempty"`
	LoadReadyCapture  string                `json:"loadReadyCapture,omitempty"`
	ReportedCapture   string                `json:"reportedCapture,omitempty"`
	FlushReadyCapture string                `json:"flushReadyCapture,omitempty"`
	LoadInput         *desktop.InputReceipt `json:"loadInput,omitempty"`
	DispatchInput     *desktop.InputReceipt `json:"dispatchInput,omitempty"`
	FlushInput        *desktop.InputReceipt `json:"flushInput,omitempty"`
	AckReadyCapture   string                `json:"ackReadyCapture,omitempty"`
	AckInput          *desktop.InputReceipt `json:"ackInput,omitempty"`
	CleanupReadyID    string                `json:"cleanupReadyId,omitempty"`
	CleanupInput      *desktop.InputReceipt `json:"cleanupInput,omitempty"`
}

type queueRetirement struct {
	// A non-nil retirement is durable intent; a non-empty revision confirms
	// the file effect. No parallel requested/completed flags can diverge.
	Revision delivery.QueueRevision `json:"revision"`
}

// ObserveReportAcknowledgement consumes new frames from the selected window's
// existing reader. The caller retains that reader across phases so its frame
// watermark is not reset. No absence of frames or input-send result is success.
// Acknowledged is deliberately not cleaned: disk cleanup is separate evidence.
func ObserveReportAcknowledgement(ctx context.Context, root, operationID string, reader *bridge.SignalReader) (evidence.CaptureRef, error) {
	if reader == nil {
		return evidence.CaptureRef{}, errors.New("live.missing_signal_reader")
	}
	return observeReportAcknowledgement(ctx, root, operationID, reader.WaitForSignal)
}

func observeReportAcknowledgement(ctx context.Context, root, operationID string, wait signalWait) (evidence.CaptureRef, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		var zero evidence.CaptureRef
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "ack_requested" || (record.Status != "running" && record.Status != "unresolved") {
			return zero, journal.ErrTransition
		}
		input, err := reportInput(record)
		if err != nil {
			return zero, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return zero, err
		}
		if observed.Schema != "lycheedev.report-observation.v1" {
			return zero, errors.New("live.invalid_report_observation")
		}
		archive := evidence.OpenArchive(store, metadata)
		report, err := archive.ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
		if err != nil {
			return zero, err
		}
		expected := input.Expected
		expected.Kind = "acknowledged"
		expected.AfterSequence = report.Receipt.Sequence
		expected.RequireInputReady = false
		signal, err := wait(ctx, expected)
		if err != nil {
			return zero, err
		}
		if signal.CodeBytes != report.Receipt.CodeBytes || signal.CodeAdler32 != report.Receipt.CodeAdler32 || signal.ReportBytes != report.Receipt.ReportBytes || signal.ReportAdler32 != report.Receipt.ReportAdler32 {
			return zero, errors.New("live.acknowledgement_payload_mismatch")
		}
		// This capture explicitly records normalized decoded signal fields, not
		// original QR text or a screenshot. Original report bytes remain separate.
		payload, err := json.Marshal(signal)
		if err != nil {
			return zero, err
		}
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(payload), MaxBytes: 4096, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-game-acknowledgement", Locator: expected.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: expected.Build, Session: expected.SessionNonce}})
		if err != nil {
			return zero, err
		}
		observed.AcknowledgementID = capture.ID
		observation, _ := json.Marshal(observed)
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "acknowledged", Status: "running", Observation: observation})
		return capture, err
	})
}

func reportInput(record journal.WorkRecord) (ReportIntent, error) {
	var input ReportIntent
	if err := json.Unmarshal(record.Intent.Request, &input); err != nil {
		return input, err
	}
	if input.Schema != "lycheedev.report-intent.v1" || record.Intent.Snapshot == "" || record.Intent.Session == "" || input.Expected.SessionNonce != record.Intent.Session {
		return input, errors.New("live.invalid_report_intent")
	}
	var bootstrap bootstrapObservation
	if len(record.Observation) != 0 {
		if err := json.Unmarshal(record.Observation, &bootstrap); err != nil {
			return input, err
		}
	}
	// Sequence floors are local to a Lua runtime. Phase receipts still impose
	// their own loaded/reported floors; the initial pre-reload floor cannot.
	if bootstrap.BootstrapCapture != "" {
		input.Expected.AfterSequence = 0
	}
	return input, nil
}

// ArchiveOperationReport advances only after both original captures exist.
// Errors may leave discoverable captures, but never ACK input or a verified stage.
func ArchiveOperationReport(ctx context.Context, root, operationID string, saved io.Reader) (evidence.ReportCaptures, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (evidence.ReportCaptures, error) {
		var zero evidence.ReportCaptures
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "persisted" || record.Status != "running" {
			return zero, journal.ErrTransition
		}
		input, err := reportInput(record)
		if err != nil {
			return zero, err
		}
		if input.Load != nil {
			return zero, errors.New("live.bound_report_source_required")
		}
		report, err := bridge.ReadPersistedReport(saved, input.Code, input.Expected)
		if err != nil {
			return zero, err
		}
		pair, err := evidence.OpenArchive(store, metadata).CommitReport(ctx, report.ReceiptBytes, report.Body, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
		if err != nil {
			return pair, err
		}
		observation, _ := json.Marshal(reportObservation{Schema: "lycheedev.report-observation.v1", BodyID: pair.Body.ID, ReceiptID: pair.Receipt.ID})
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "verified", Status: "running", Observation: observation})
		return pair, err
	})
}

// ArchiveInstalledOperationReport selects the source from frozen operation
// intent, not a new caller path. The persisted stage must already be supported
// by the coordinator's flush evidence; this read alone cannot establish it.
func ArchiveInstalledOperationReport(ctx context.Context, root, operationID string) (evidence.ReportCaptures, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (evidence.ReportCaptures, error) {
		var zero evidence.ReportCaptures
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "persisted" || record.Status != "running" {
			return zero, journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		signal, err := reportedOperationEvidence(ctx, evidence.OpenArchive(store, metadata), record, input, definition)
		if err != nil {
			return zero, err
		}
		installed, err := ReadInstalledReport(ctx, input.Load.Installation, input.Load.Account, input.Code, input.Expected)
		if err != nil {
			return zero, err
		}
		if installed.Report.Receipt != signal {
			return zero, errors.New("live.persisted_receipt_mismatch")
		}
		pair, err := evidence.OpenArchive(store, metadata).CommitReport(ctx, installed.Report.ReceiptBytes, installed.Report.Body, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
		if err != nil {
			return pair, err
		}
		var prior probeLoadObservation
		if err := json.Unmarshal(record.Observation, &prior); err != nil {
			return zero, err
		}
		observation, _ := json.Marshal(reportObservation{
			bootstrapObservation: prior.bootstrapObservation, Schema: "lycheedev.report-observation.v1",
			BodyID: pair.Body.ID, ReceiptID: pair.Receipt.ID, SourcePath: installed.Path, SourceSHA256: installed.FileSHA256,
			ReloadedCapture: prior.ReloadedCapture, LoadedCapture: prior.LoadedCapture,
			LoadReadyCapture: prior.LoadReadyCapture, ReportedCapture: prior.ReportedCapture,
			FlushReadyCapture: prior.FlushReadyCapture, LoadInput: prior.LoadInput,
			DispatchInput: prior.DispatchInput, FlushInput: prior.FlushInput,
		})
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "verified", Status: "running", Observation: observation})
		return pair, err
	})
}

// RequestReportAcknowledgement persists intent before returning sendable data.
// It deliberately rejects ack_requested retries: an interrupted external effect
// must first be reconciled with fresh game evidence, never blindly replayed.
func RequestReportAcknowledgement(ctx context.Context, root, operationID string) (bridge.VerifiedReport, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (bridge.VerifiedReport, error) {
		var zero bridge.VerifiedReport
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "verified" || record.Status != "running" {
			return zero, journal.ErrTransition
		}
		input, err := reportInput(record)
		if err != nil {
			return zero, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return zero, err
		}
		if observed.Schema != "lycheedev.report-observation.v1" {
			return zero, errors.New("live.invalid_report_observation")
		}
		report, err := evidence.OpenArchive(store, metadata).ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
		if err != nil {
			return zero, err
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "ack_requested", Status: "running", Observation: record.Observation}); err != nil {
			return zero, err
		}
		return report, nil
	})
}
