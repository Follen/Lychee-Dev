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
)

// ObserveRemoval archives the complete selected account file after a verified
// ACK, only if the original request is absent. It does not send reload, delete
// files, prove reload completion, mark cleaned, or release durable ownership.
func (p *ProbeOperation) ObserveRemoval(ctx context.Context) (evidence.CaptureRef, error) {
	var zero evidence.CaptureRef
	if err := p.check(ctx); err != nil {
		return zero, err
	}
	return withOperation(ctx, p.root, p.id, func(store *vault.Store, metadata *vault.Metadata) (evidence.CaptureRef, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, p.id)
		if err != nil {
			return zero, err
		}
		if record.Stage != "acknowledged" || record.Status != "running" && record.Status != "unresolved" {
			return zero, journal.ErrTransition
		}
		input, _, err := probeDefinition(record)
		if err != nil {
			return zero, err
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return zero, err
		}
		archive := evidence.OpenArchive(store, metadata)
		report, err := acknowledgedReport(ctx, archive, record, input, observed)
		if err != nil {
			return zero, err
		}
		state, err := readInstalledState(ctx, input.Load.Installation, input.Load.Account, input.Expected)
		if err != nil {
			return zero, err
		}
		if state.Path != observed.SourcePath || state.FileSHA256 == observed.SourceSHA256 {
			return zero, errors.New("live.removal_source_mismatch")
		}
		if _, err := bridge.VerifyArchivedReportRemoval(report.ReceiptBytes, report.Body, bytes.NewReader(state.Bytes), input.Code, input.Expected); err != nil {
			return zero, err
		}
		if err := p.check(ctx); err != nil {
			return zero, err
		}
		want := evidence.Provenance{Kind: "game-report-removal", Locator: state.Path, Snapshot: record.Intent.Snapshot, OperationID: p.id, DataBuild: input.Expected.Build, Session: input.Expected.SessionNonce}
		if observed.RemovalID != "" && observed.RemovalSHA256 == state.FileSHA256 {
			previous, data, err := archive.FetchCapture(ctx, observed.RemovalID, bridge.SavedStateLimits().FileBytes)
			if err != nil {
				return zero, err
			}
			if previous.Provenance != want || !previous.Complete || previous.Truncated || previous.MediaType != "text/plain" || !bytes.Equal(data, state.Bytes) {
				return zero, errors.New("live.removal_provenance_mismatch")
			}
			return previous, nil
		}
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(state.Bytes), MaxBytes: bridge.SavedStateLimits().FileBytes, MediaType: "text/plain", Complete: true, Provenance: want})
		if err != nil {
			return zero, err
		}
		observed.RemovalID, observed.RemovalSHA256 = capture.ID, state.FileSHA256
		observation, _ := json.Marshal(observed)
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: p.id, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "acknowledged", Status: "running", Observation: observation})
		return capture, err
	})
}
