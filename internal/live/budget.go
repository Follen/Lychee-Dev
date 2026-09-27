package live

import (
	"encoding/json"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Receiver attempts have their own 60 second input bound. An operation budget
// must also leave room for a full execution, the SV reload, and cleanup.
const (
	receiverPhaseBudget     = 60 * time.Second
	resultPersistenceBudget = 90 * time.Second
	executionReceiptGrace   = 20 * time.Second
	// Load can reconnect (identity and connect), then prepare and load through
	// four independently bounded receiver transactions. Leave time for the
	// intervening ready/reentry observations and durable file checks.
	loadPhaseBudget = 4*receiverPhaseBudget + 2*15*time.Second + resultPersistenceBudget
	// First connect and reset each need three receiver transactions; standalone
	// reload and finish-display cleanup can reconnect twice before their own
	// refresh or hide command.
	bootstrapLifecycleBudget = 3*receiverPhaseBudget + 3*15*time.Second + resultPersistenceBudget
)

func executionBudget(record journal.WorkRecord) time.Duration {
	input, err := reportInput(record)
	if err != nil {
		return 120 * time.Second
	}
	return time.Duration(input.BudgetSeconds) * time.Second
}

// The execution clock starts with durable dispatch intent, before the first
// possible game input. A resumed attempt never receives another full run.
func executionRemaining(record journal.WorkRecord, now time.Time) time.Duration {
	var observed struct {
		ExecutionRequestedAt time.Time `json:"executionRequestedAt"`
	}
	if json.Unmarshal(record.Observation, &observed) != nil || observed.ExecutionRequestedAt.IsZero() {
		return 0
	}
	return observed.ExecutionRequestedAt.Add(receiverPhaseBudget + executionBudget(record) + executionReceiptGrace).Sub(now)
}

func lifecycleBudget(record journal.WorkRecord, now time.Time) time.Duration {
	switch record.Stage {
	case "loaded":
		return receiverPhaseBudget + executionBudget(record) + executionReceiptGrace + resultPersistenceBudget
	case "dispatch_requested":
		remaining := executionRemaining(record, now)
		if remaining < 0 {
			remaining = 0
		}
		return remaining + resultPersistenceBudget
	case "reported", "flush_requested", "persisted", "verified", "ack_requested", "acknowledged":
		return receiverPhaseBudget + resultPersistenceBudget
	default:
		return loadPhaseBudget + executionBudget(record) + resultPersistenceBudget
	}
}
