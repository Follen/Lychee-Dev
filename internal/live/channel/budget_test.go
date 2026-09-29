package channel

import (
	"encoding/json"
	"errors"
	"testing"
	"time"
)

func TestBudgetResumeAfterRestartKeepsOriginalDeadline(t *testing.T) {
	start := time.Unix(1800000000, 0)
	b := NewDurableBudget(start, DefaultRecoveryBudget, false)
	if remaining, changed, err := b.Observe(start.Add(100 * time.Second)); err != nil || !changed || remaining != 500*time.Second {
		t.Fatalf("remaining=%v changed=%v err=%v", remaining, changed, err)
	}
	data, err := json.Marshal(b)
	if err != nil {
		t.Fatal(err)
	}
	var resumed DurableBudget
	if err := json.Unmarshal(data, &resumed); err != nil {
		t.Fatal(err)
	}
	if remaining, _, err := resumed.Observe(start.Add(590 * time.Second)); err != nil || remaining != 10*time.Second {
		t.Fatalf("resume granted time: remaining=%v err=%v", remaining, err)
	}
	if _, changed, err := resumed.Observe(start.Add(DefaultRecoveryBudget)); !changed || !errors.Is(err, ErrBudgetExhausted) || !errors.Is(err, ErrPending) {
		t.Fatalf("deadline did not stop work: changed=%v err=%v", changed, err)
	}
	if _, changed, err := resumed.Observe(start.Add(time.Second)); changed || !errors.Is(err, ErrBudgetExhausted) {
		t.Fatalf("exhausted budget revived: changed=%v err=%v", changed, err)
	}
}

func TestBudgetClockRollbackRemainsStoppedAfterSerialization(t *testing.T) {
	start := time.Unix(1800000000, 0)
	b := NewDurableBudget(start, DefaultRecoveryBudget, false)
	_, _, _ = b.Observe(start.Add(10 * time.Second))
	if _, changed, err := b.Observe(start.Add(9 * time.Second)); !changed || !errors.Is(err, ErrBudgetClockRollback) || !errors.Is(err, ErrBudgetExhausted) || !errors.Is(err, ErrPending) {
		t.Fatalf("rollback accepted: changed=%v err=%v", changed, err)
	}
	data, err := json.Marshal(b)
	if err != nil {
		t.Fatal(err)
	}
	var resumed DurableBudget
	if err := json.Unmarshal(data, &resumed); err != nil {
		t.Fatal(err)
	}
	if _, changed, err := resumed.Observe(start.Add(20 * time.Second)); changed || !errors.Is(err, ErrBudgetClockRollback) {
		t.Fatalf("rollback forgotten: changed=%v err=%v", changed, err)
	}
}

func TestBusinessExhaustionDoesNotConsumeCloseBudget(t *testing.T) {
	start := time.Unix(1800000000, 0)
	s := State{Operation: &Operation{RecoveryBudget: NewDurableBudget(start, DefaultRecoveryBudget, false)}}
	closeStart := start.Add(DefaultRecoveryBudget)
	_, _, _ = s.Operation.RecoveryBudget.Observe(closeStart)
	s.CloseBudget = NewDurableBudget(closeStart, DefaultCloseBudget, false)
	if remaining, changed, err := s.CloseBudget.Observe(closeStart); err != nil || changed || remaining != DefaultCloseBudget {
		t.Fatalf("close lost its reserve: remaining=%v changed=%v err=%v", remaining, changed, err)
	}
	if _, _, err := s.CloseBudget.Observe(closeStart.Add(DefaultCloseBudget)); !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal("close budget was unbounded", err)
	}
}

func TestLegacyBudgetsRemainAbsentUntilExplicitMigration(t *testing.T) {
	var s State
	if err := json.Unmarshal([]byte(`{"schema":"lycheedev.channel.v1","operation":{"budget":30}}`), &s); err != nil {
		t.Fatal(err)
	}
	if err := validateBudgets(s); err != nil {
		t.Fatal(err)
	}
	if s.ConnectBudget != nil || s.CloseBudget != nil || s.Operation.RecoveryBudget != nil {
		t.Fatal("read silently granted a budget")
	}
	s.Operation.RecoveryBudget = NewDurableBudget(time.Unix(1800000000, 0), DefaultRecoveryBudget, true)
	if err := validateBudgets(s); err != nil {
		t.Fatal(err)
	}
	if !s.Operation.RecoveryBudget.Legacy {
		t.Fatal("migration origin lost")
	}
}

func TestBudgetValidationRejectsCorruptWindows(t *testing.T) {
	start := time.Unix(1800000000, 0)
	for _, mutate := range []func(*DurableBudget){
		func(b *DurableBudget) { b.DeadlineMS = b.StartedAtMS },
		func(b *DurableBudget) { b.LastObservedMS = b.StartedAtMS - 1 },
		func(b *DurableBudget) { b.LastObservedMS = b.DeadlineMS },
		func(b *DurableBudget) { b.ClockRollback = true },
		func(b *DurableBudget) { b.DeadlineMS = int64(^uint64(0) >> 1) },
	} {
		b := NewDurableBudget(start, DefaultConnectBudget, false)
		mutate(b)
		if err := validateBudgets(State{ConnectBudget: b}); err == nil {
			t.Fatalf("corrupt budget accepted: %+v", b)
		}
	}
}
