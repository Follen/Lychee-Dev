package channel

import (
	"context"
	"errors"
	"time"
)

type Continuation struct {
	Kind              string   `json:"kind"`
	Session           string   `json:"session"`
	RequestID         string   `json:"requestId,omitempty"`
	Goal              string   `json:"goal,omitempty"`
	ProgressVersion   uint64   `json:"progressVersion"`
	RemainingBudgetMS *int64   `json:"remainingBudgetMs,omitempty"`
	Blocker           *Blocker `json:"blocker,omitempty"`
}

// This is a projection of existing facts, never a second action selector.
func (d *Driver) continuation() Continuation {
	c := Continuation{Kind: "continue", Session: d.State.ID, ProgressVersion: d.State.ProgressVersion, Blocker: d.State.Blocker}
	if d.State.RuntimeEnd != nil && !d.State.Closed {
		c.Goal, c.RequestID = "close", "close:"+d.State.ID
		c.Blocker = &Blocker{Kind: "runtime_retirement_pending", Condition: "precise_resource_retirement"}
		return c
	}
	ref, _, goal := d.goalBudget()
	c.Goal = goal
	if op := d.State.Operation; op != nil {
		c.RequestID = op.Request
	}
	if goal == "reload" {
		c.RequestID = d.State.Reload.Request
	}
	if goal == "close" {
		c.RequestID = "close:" + d.State.ID
	}
	// Terminal evidence wins over a blocker saved by an earlier invocation,
	// including a crash after terminal Save but before the deferred projection.
	if !d.State.Closing && d.State.Operation != nil && d.State.Operation.Stage == "execution_unknown" && goal == "" {
		c.Kind, c.Blocker = "needs_decision", nil
		return c
	}
	if goal == "" && (d.State.Closed || d.State.Bound && d.State.Transaction == nil && (d.State.Operation == nil || d.State.Operation.Stage == "complete" || d.State.Operation.Stage == "cancelled") && (d.State.Recovery == nil || d.State.Recovery.Phase == "complete")) {
		c.Kind, c.Blocker = "completed", nil
		return c
	}
	if ref != nil && *ref != nil {
		b := *ref
		remaining := b.DeadlineMS - d.now().UnixMilli()
		if b.Exhausted || remaining < 0 {
			remaining = 0
		}
		c.RemainingBudgetMS = &remaining
		if remaining == 0 {
			c.Kind = "budget_exhausted"
			return c
		}
	}
	if b := c.Blocker; b != nil {
		switch b.Kind {
		case "active_driver":
			c.Kind = "wait_active_driver"
		case "budget_exhausted", "budget_clock_rollback":
			c.Kind = "budget_exhausted"
		case "prepared_operation_requires_reload", "runtime_changed_during_recovery":
			c.Kind = "needs_decision"
		case "receipt_pending", "input_keyboard_focus":
			c.Kind = "continue"
		default:
			c.Kind = "wait_external"
		}
		return c
	}
	return c
}

// Persist only a changed dependency; repeated observations are not progress.
func (d *Driver) recordBlocker(ctx context.Context, result error) error {
	var next *Blocker
	var blocked *BlockedError
	if errors.Is(result, ErrBudgetExhausted) {
		kind := "budget_exhausted"
		if errors.Is(result, ErrBudgetClockRollback) {
			kind = "budget_clock_rollback"
		}
		next = &Blocker{Kind: kind, Condition: "explicit_budget_decision"}
	} else if errors.As(result, &blocked) {
		copy := blocked.Blocker
		next = &copy
	} else if errors.Is(result, ErrPending) {
		kind, condition := d.Waiting, "input_condition_changed"
		if kind == "" {
			kind, condition = "receipt_pending", "exact_receipt_or_runtime_change"
		}
		switch kind {
		case "prepared_operation_requires_reload":
			condition = "authorized_reload_or_runtime_destruction"
		case "closing_exchange_unconfirmed":
			condition = "exact_receipt_or_runtime_destruction"
		case "budget_exhausted", "budget_clock_rollback":
			condition = "explicit_budget_decision"
		case "shared_publication":
			condition = "publication_dependency_changed"
		}
		next = &Blocker{Kind: kind, Runtime: d.State.Identity.Runtime, Condition: condition}
		if tx := d.State.Transaction; tx != nil {
			next.Slot = tx.Envelope.Index
			next.Nonce = tx.Envelope.Nonce
		}
	}
	old := d.State.Blocker
	if old == nil && next == nil || old != nil && next != nil && *old == *next {
		return nil
	}
	d.State.Blocker = next
	flush, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
	defer cancel()
	return d.Save(flush, "blocker_observed")
}
