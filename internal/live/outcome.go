package live

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Outcome is a read model, not another persisted state machine. A verified
// report remains usable even when the game's cleanup has not been confirmed.
type Outcome struct {
	OperationID    string           `json:"operationId"`
	Snapshot       string           `json:"snapshot"`
	Status         string           `json:"status"`
	Complete       bool             `json:"complete"`
	Goal           string           `json:"goal,omitempty"`
	NextAction     *NextAction      `json:"nextAction,omitempty"`
	RuntimeCapture string           `json:"runtimeCapture,omitempty"`
	Report         ReportOutcome    `json:"report"`
	Business       *BusinessOutcome `json:"business,omitempty"`
	Cleanup        string           `json:"cleanup"`
	Display        *DisplayOutcome  `json:"display,omitempty"`
	Stage          string           `json:"-"`
}

type NextAction struct {
	Command string   `json:"command"`
	Args    []string `json:"args"`
	Reason  string   `json:"reason"`
}

var ErrBusinessFailed = errors.New("live.business_failed")

func withNextAction(result Outcome) Outcome {
	if result.Goal == "finished" && !result.Complete && result.Cleanup != "abandoned" && result.Status != "failed" {
		result.NextAction = &NextAction{Command: "lycheedev", Args: []string{"live", "resume", result.OperationID}, Reason: "continue_same_operation_without_replaying_unknown_input"}
	}
	return result
}

type ReportOutcome struct {
	State          string          `json:"state"`
	ErrorCode      string          `json:"errorCode,omitempty"`
	ErrorCapture   string          `json:"errorCapture,omitempty"`
	BodyCapture    string          `json:"bodyCapture,omitempty"`
	ReceiptCapture string          `json:"receiptCapture,omitempty"`
	SHA256         string          `json:"sha256,omitempty"`
	Content        json.RawMessage `json:"content,omitempty"`
}

type BusinessOutcome struct {
	State                 string `json:"state"`
	AcceptedBudgetSeconds int    `json:"acceptedBudgetSeconds,omitempty"`
	AssertionsPassed      *bool  `json:"assertionsPassed,omitempty"`
}

func businessOutcome(body []byte, input ReportIntent) (*BusinessOutcome, error) {
	if input.Revision == "builtin:bugs" {
		return nil, nil
	}
	var report struct {
		ProbeStatus           string          `json:"probeStatus"`
		AcceptedBudgetSeconds int             `json:"acceptedBudgetSeconds"`
		Result                json.RawMessage `json:"result"`
	}
	if err := json.Unmarshal(body, &report); err != nil {
		return nil, err
	}
	if report.ProbeStatus != "completed" && report.ProbeStatus != "failed" {
		if !input.DeclaredBudget {
			return nil, nil
		}
		return nil, errors.New("live.business_status_invalid")
	}
	if input.DeclaredBudget && report.AcceptedBudgetSeconds != input.BudgetSeconds {
		return nil, errors.New("live.execution_budget_mismatch")
	}
	result := &BusinessOutcome{State: report.ProbeStatus, AcceptedBudgetSeconds: report.AcceptedBudgetSeconds}
	if report.ProbeStatus == "completed" && len(report.Result) > 0 {
		var asserted struct {
			Passed *bool `json:"passed"`
		}
		if json.Unmarshal(report.Result, &asserted) == nil && asserted.Passed != nil {
			result.AssertionsPassed = asserted.Passed
			if *asserted.Passed {
				result.State = "passed"
			} else {
				result.State = "assertion_failed"
			}
		}
	}
	return result, nil
}

// Status reads and verifies available results without connecting to a game.
func Status(ctx context.Context, root, id string) (Outcome, error) {
	result, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (Outcome, error) {
		record, err := journal.OpenBook(metadata).InspectWork(ctx, id)
		if err != nil {
			return Outcome{}, err
		}
		result := pendingOutcome(record)
		if record.Intent.Kind == "reload" {
			var kind struct {
				Schema string `json:"schema"`
			}
			if json.Unmarshal(record.Intent.Request, &kind) == nil && kind.Schema == "lycheedev.fixed-reload.v1" {
				return fixedReloadOutcome(ctx, evidence.OpenArchive(store, metadata), record, result)
			}
			return reloadOutcome(ctx, evidence.OpenArchive(store, metadata), record, result)
		}
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return result, err
		}
		if record.Status == "failed" && record.Stage == "dispatch_requested" {
			var failure probeLoadObservation
			if err := json.Unmarshal(record.Observation, &failure); err != nil {
				return result, err
			}
			if failure.ReportErrorCapture == "" {
				return result, nil
			}
			ref, raw, err := evidence.OpenArchive(store, metadata).FetchCapture(ctx, failure.ReportErrorCapture, 4096)
			if err != nil {
				return result, err
			}
			input, err := reportInput(record)
			if err != nil {
				return result, err
			}
			if ref.Provenance.Kind != "decoded-game-report-error" || ref.Provenance.OperationID != record.OperationID || ref.Provenance.Locator != input.Expected.RequestID || !ref.Complete || ref.Truncated {
				return result, errors.New("live.report_error_evidence_mismatch")
			}
			signal, err := bridge.ParseSignal(raw)
			if err != nil {
				return result, err
			}
			if signal.Kind != "report_error" || signal.RequestID != input.Expected.RequestID || signal.ErrorCode == "" || signal.SessionNonce != input.Expected.SessionNonce {
				return result, errors.New("live.report_error_evidence_mismatch")
			}
			result.Report = ReportOutcome{State: "unavailable", ErrorCode: signal.ErrorCode, ErrorCapture: ref.ID}
			return result, nil
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
		result.Business, err = businessOutcome(report.Body, input)
		if err != nil {
			return result, err
		}
		result.Complete = result.Cleanup == "complete"
		if record.Intent.Goal == "finished" {
			result.Display = &DisplayOutcome{State: "pending"}
			result.Complete = false
			display, err := readDisplayProof(ctx, store, metadata, record, input.Binding)
			if err != nil {
				return result, err
			}
			if display.State == "cleared" {
				result.Display = &display
				result.Complete = result.Cleanup == "complete"
			}
		}
		return result, nil
	})
	return withNextAction(result), err
}

func pendingOutcome(record journal.WorkRecord) Outcome {
	result := Outcome{OperationID: record.OperationID, Snapshot: record.Intent.Snapshot, Status: record.Status, Stage: record.Stage, Goal: record.Intent.Goal, Report: ReportOutcome{State: "unavailable"}, Cleanup: "pending"}
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
	if record.Intent.Goal == "finished" && result.Complete && result.Business != nil && (result.Business.State == "failed" || result.Business.State == "assertion_failed") {
		runErr = errors.Join(runErr, ErrBusinessFailed)
	}
	if record.Intent.Goal == "finished" && !result.Complete && result.Cleanup != "abandoned" && runErr == nil && err == nil {
		runErr = ErrInvestigationPending
	}
	return withNextAction(result), errors.Join(runErr, err)
}
