package live

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Outcome is a read model, not another persisted state machine. A verified
// report remains usable even when the game's cleanup has not been confirmed.
type Outcome struct {
	OperationID    string        `json:"operationId"`
	Snapshot       string        `json:"snapshot"`
	Status         string        `json:"status"`
	Complete       bool          `json:"complete"`
	RuntimeCapture string        `json:"runtimeCapture,omitempty"`
	Report         ReportOutcome `json:"report"`
	Cleanup        string        `json:"cleanup"`
	Stage          string        `json:"-"`
}

type ReportOutcome struct {
	State          string          `json:"state"`
	BodyCapture    string          `json:"bodyCapture,omitempty"`
	ReceiptCapture string          `json:"receiptCapture,omitempty"`
	SHA256         string          `json:"sha256,omitempty"`
	Content        json.RawMessage `json:"content,omitempty"`
}

// Status reads and verifies available results without connecting to a game.
func Status(ctx context.Context, root, id string) (Outcome, error) {
	return vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (Outcome, error) {
		record, err := journal.OpenBook(metadata).InspectWork(ctx, id)
		if err != nil {
			return Outcome{}, err
		}
		result := pendingOutcome(record)
		if record.Intent.Kind == "reload" {
			return reloadOutcome(ctx, evidence.OpenArchive(store, metadata), record, result)
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return result, err
		}
		if observed.BodyID == "" || observed.ReceiptID == "" {
			return result, nil
		}
		input, err := reportInput(record)
		if err != nil {
			return result, err
		}
		report, err := evidence.OpenArchive(store, metadata).ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, record.OperationID, record.Intent.Snapshot)
		if err != nil {
			return result, err
		}
		result.Report = ReportOutcome{State: "verified", BodyCapture: observed.BodyID, ReceiptCapture: observed.ReceiptID, SHA256: report.SHA256, Content: json.RawMessage(report.Body)}
		result.Complete = result.Cleanup == "complete"
		return result, nil
	})
}

func pendingOutcome(record journal.WorkRecord) Outcome {
	result := Outcome{OperationID: record.OperationID, Snapshot: record.Intent.Snapshot, Status: record.Status, Stage: record.Stage, Report: ReportOutcome{State: "unavailable"}, Cleanup: "pending"}
	if record.Stage == "abandoned" && record.Status == "abandoned" {
		result.Cleanup = "abandoned"
	}
	if record.Stage == "cleaned" && (record.Status == "completed" || record.Status == "cancelled") {
		result.Cleanup = "complete"
	}
	return result
}

func finishOutcome(ctx context.Context, root string, record journal.WorkRecord, runErr error) (Outcome, error) {
	if record.OperationID == "" {
		return Outcome{}, runErr
	}
	// The run's deadline cannot discard an already committed result. This
	// bounded read performs no game input or state transition.
	read, cancel := context.WithTimeout(context.WithoutCancel(ctx), 2*time.Second)
	defer cancel()
	result, err := Status(read, root, record.OperationID)
	if result.OperationID == "" {
		result = pendingOutcome(record)
	}
	return result, errors.Join(runErr, err)
}
