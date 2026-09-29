package channel

import (
	"errors"
	"time"
)

const (
	DefaultRecoveryBudget = 600 * time.Second
	DefaultConnectBudget  = 120 * time.Second
	DefaultCloseBudget    = 120 * time.Second
)

var (
	ErrBudgetExhausted     = errors.Join(ErrPending, errors.New("live.channel_budget_exhausted"))
	ErrBudgetClockRollback = errors.Join(ErrBudgetExhausted, errors.New("live.channel_budget_clock_rollback"))
)

// DurableBudget is an absolute wall-clock window for one immutable goal. Idle
// time between CLI calls counts. It is not the Lua execution budget. The caller
// persists initialization and observations before new input, and caps its local
// monotonic context deadline by the remaining duration.
//
// Legacy marks a single migration window granted on the first drive of an older
// journal. Reads never migrate. Prior elapsed time cannot be reconstructed.
// LastObservedMS detects observed clock rollback; it cannot detect an unseen
// backward/forward clock change while the host is offline.
type DurableBudget struct {
	StartedAtMS    int64 `json:"startedAtMs"`
	DeadlineMS     int64 `json:"deadlineMs"`
	LastObservedMS int64 `json:"lastObservedMs"`
	Exhausted      bool  `json:"exhausted,omitempty"`
	ClockRollback  bool  `json:"clockRollback,omitempty"`
	Legacy         bool  `json:"legacy,omitempty"`
}

func NewDurableBudget(now time.Time, duration time.Duration, legacy bool) *DurableBudget {
	start := now.UnixMilli()
	return &DurableBudget{StartedAtMS: start, DeadlineMS: start + duration.Milliseconds(), LastObservedMS: start, Legacy: legacy}
}

// Observe updates only the in-memory budget. changed requires a durable save
// before continuing automatic work. Exhaustion is sticky, including rollback:
// restoring the wall clock must not revive a stopped goal.
func (b *DurableBudget) Observe(now time.Time) (remaining time.Duration, changed bool, err error) {
	if b == nil || b.validate() != nil {
		return 0, false, errors.New("live.channel_budget_invalid")
	}
	if b.Exhausted {
		if b.ClockRollback {
			return 0, false, ErrBudgetClockRollback
		}
		return 0, false, ErrBudgetExhausted
	}
	n := now.UnixMilli()
	if n < b.LastObservedMS {
		b.Exhausted, b.ClockRollback = true, true
		return 0, true, ErrBudgetClockRollback
	}
	changed = n != b.LastObservedMS
	b.LastObservedMS = n
	if n >= b.DeadlineMS {
		b.Exhausted = true
		return 0, true, ErrBudgetExhausted
	}
	return time.Duration(b.DeadlineMS-n) * time.Millisecond, changed, nil
}

func (b *DurableBudget) validate() error {
	if b == nil {
		return nil // Older journals are migrated only when driven.
	}
	// A duration that cannot fit in time.Duration must never wrap the caller's
	// deadline. Production defaults are much smaller than this encoding limit.
	const maxMS = int64(^uint64(0)>>1) / int64(time.Millisecond)
	if b.StartedAtMS <= 0 || b.DeadlineMS <= b.StartedAtMS || b.DeadlineMS-b.StartedAtMS > maxMS || b.LastObservedMS < b.StartedAtMS || b.ClockRollback && !b.Exhausted || !b.Exhausted && b.LastObservedMS >= b.DeadlineMS {
		return errors.New("live.channel_budget_invalid")
	}
	return nil
}

func validateBudgets(s State) error {
	budgets := []*DurableBudget{s.ConnectBudget, s.CloseBudget}
	if s.Operation != nil {
		budgets = append(budgets, s.Operation.RecoveryBudget)
	}
	if s.Reload != nil {
		budgets = append(budgets, s.Reload.RecoveryBudget)
	}
	for _, b := range budgets {
		if err := b.validate(); err != nil {
			return err
		}
	}
	return nil
}
