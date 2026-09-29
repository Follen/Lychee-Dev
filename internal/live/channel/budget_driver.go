package channel

import (
	"context"
	"errors"
	"time"
)

func (d *Driver) now() time.Time {
	if d.Now != nil {
		return d.Now()
	}
	return time.Now()
}

// A goal owns its deadline; a CLI invocation only borrows its remaining time.
// Close has its own reserve. Automatic reload inherits the business deadline.
func (d *Driver) goalBudget() (**DurableBudget, time.Duration, string) {
	s := &d.State
	// An explicit control request owns its budget even while an older close waits.
	if s.Reload != nil && !s.Reload.Automatic && s.Reload.Phase != "complete" {
		return &s.Reload.RecoveryBudget, DefaultRecoveryBudget, "reload"
	}
	if s.Closing {
		return &s.CloseBudget, DefaultCloseBudget, "close"
	}
	if s.Reload != nil && s.Reload.Phase != "complete" {
		return &s.Reload.RecoveryBudget, DefaultRecoveryBudget, "reload"
	}
	if s.Operation != nil && s.Operation.Stage != "complete" && s.Operation.Stage != "cancelled" && s.Operation.Stage != "execution_unknown" {
		return &s.Operation.RecoveryBudget, DefaultRecoveryBudget, "operation"
	}
	if !s.Bound && !s.Closed {
		return &s.ConnectBudget, DefaultConnectBudget, "connect"
	}
	return nil, 0, ""
}

func (d *Driver) budgetContext(ctx context.Context) (context.Context, context.CancelFunc, error) {
	ref, duration, _ := d.goalBudget()
	if ref == nil {
		return ctx, func() {}, nil
	}
	kind := "budget_observed"
	created := false
	if *ref == nil {
		*ref = NewDurableBudget(d.now(), duration, true)
		kind = "budget_migrated"
		created = true
	}
	remaining, changed, budgetErr := (*ref).Observe(d.now())
	// Save even a stopped goal, so restoring the clock cannot revive it.
	if created || changed {
		if err := d.Save(ctx, kind); err != nil {
			return ctx, func() {}, err
		}
	}
	if budgetErr != nil {
		d.Waiting = "budget_exhausted"
		if errors.Is(budgetErr, ErrBudgetClockRollback) {
			d.Waiting = "budget_clock_rollback"
		}
		return ctx, func() {}, budgetErr
	}
	bounded, cancel := context.WithTimeoutCause(ctx, remaining, ErrBudgetExhausted)
	return bounded, cancel, nil
}

func (d *Driver) finishBudget(ctx context.Context) error {
	if !errors.Is(context.Cause(ctx), ErrBudgetExhausted) {
		return nil
	}
	d.Waiting = "budget_exhausted"
	ref, _, _ := d.goalBudget()
	if ref == nil || *ref == nil || (*ref).Exhausted {
		return nil
	}
	(*ref).Exhausted = true
	flush, cancel := context.WithTimeout(context.WithoutCancel(ctx), 3*time.Second)
	defer cancel()
	return d.Save(flush, "budget_observed")
}

func (d *Driver) RequestClose(ctx context.Context) error {
	if d.State.Closed {
		return nil
	}
	if d.State.CloseBudget == nil {
		d.State.CloseBudget = NewDurableBudget(d.now(), DefaultCloseBudget, false)
	}
	if d.State.Closing {
		return nil
	}
	d.State.Closing = true
	return d.Save(ctx, "disconnect_intent")
}
