package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func reportedOperationEvidence(ctx context.Context, archive *evidence.Archive, record journal.WorkRecord, input ReportIntent, definition bridge.ProbeDefinition) (bridge.Signal, error) {
	var zero bridge.Signal
	observed, loaded, err := loadedOperationEvidence(ctx, archive, record, input, definition)
	if err != nil {
		return zero, err
	}
	if observed.ReportedCapture == "" {
		return zero, errors.New("live.reported_evidence_missing")
	}
	ref, data, err := archive.FetchCapture(ctx, observed.ReportedCapture, 4096)
	if err != nil {
		return zero, err
	}
	want := evidence.Provenance{Kind: "decoded-game-reported", Locator: definition.RequestID, Snapshot: record.Intent.Snapshot, OperationID: record.OperationID, DataBuild: definition.Build, Session: definition.SessionNonce}
	if ref.Provenance != want || !ref.Complete || ref.Truncated || ref.MediaType != "application/json" {
		return zero, errors.New("live.reported_evidence_mismatch")
	}
	signal, err := bridge.ParseSignal(data)
	if err != nil {
		return zero, err
	}
	expected := input.Expected
	expected.Kind, expected.AfterSequence, expected.ReloadNonce, expected.RequireInputReady = "reported", loaded.Sequence, "", false
	if err := signal.Match(expected); err != nil {
		return zero, err
	}
	if signal.ReloadNonce != "" || signal.CodeBytes != loaded.CodeBytes || signal.CodeAdler32 != loaded.CodeAdler32 {
		return zero, errors.New("live.reported_code_mismatch")
	}
	return signal, nil
}

// RequestOperationFlush records intent before returning a reload command. It
// never sends input or claims persistence. A repeated call cannot replay reload;
// the live sender must independently check its window lease and eligibility.
func RequestOperationFlush(ctx context.Context, root, operationID string) (string, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (string, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return "", err
		}
		if record.Stage != "reported" || record.Status != "running" {
			return "", journal.ErrTransition
		}
		input, definition, err := probeDefinition(record)
		if err != nil {
			return "", err
		}
		if _, err := reportedOperationEvidence(ctx, evidence.OpenArchive(store, metadata), record, input, definition); err != nil {
			return "", err
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "flush_requested", Status: "running", Observation: record.Observation}); err != nil {
			return "", err
		}
		return "/dev bridge reload " + definition.RequestID, nil
	})
}

// ObserveInstalledOperationPersisted proves only that the selected account file
// contains the exact observed report and valid body. It does not prove a reload
// completed or that the client is ready again. No file timestamp substitutes for
// the original receipt, code and body checks.
func ObserveInstalledOperationPersisted(ctx context.Context, root, operationID string) (InstalledReport, error) {
	return withOperation(ctx, root, operationID, func(store *vault.Store, metadata *vault.Metadata) (InstalledReport, error) {
		var zero InstalledReport
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, operationID)
		if err != nil {
			return zero, err
		}
		if record.Stage != "flush_requested" || (record.Status != "running" && record.Status != "unresolved") {
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
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: "persisted", Status: "running", Observation: record.Observation})
		return installed, err
	})
}
