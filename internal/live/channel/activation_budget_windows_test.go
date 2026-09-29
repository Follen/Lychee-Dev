//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

func TestActivationBudgetLegacyReadAndDrive(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	a := activation{Schema: "lycheedev.channel-activation.v1", Request: "activation", Phase: "prepared"}
	ctx := context.Background()
	if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(p.activationPath(d.State.ID))
	r, err := p.activationStatus(d.State.ID)
	if err != nil || r.Continuation.RemainingBudgetMS != nil {
		t.Fatal("read migrated legacy budget", r, err)
	}
	after, _ := os.ReadFile(p.activationPath(d.State.ID))
	if string(before) != string(after) {
		t.Fatal("status wrote activation")
	}
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	now := time.Now()
	remaining, err := p.observeActivationBudget(ctx, d.State.ID, &a, now)
	if err != nil || remaining != DefaultRecoveryBudget || !a.Budget.Legacy || a.Schema != "lycheedev.channel-activation.v2" {
		t.Fatal(a, remaining, err)
	}
	deadline := a.Budget.DeadlineMS
	var resumed activation
	if err := readProjectJSON(p.activationPath(d.State.ID), &resumed, 16384); err != nil {
		t.Fatal(err)
	}
	remaining, err = p.observeActivationBudget(ctx, d.State.ID, &resumed, now.Add(time.Minute))
	if err != nil || resumed.Budget.DeadlineMS != deadline || remaining != DefaultRecoveryBudget-time.Minute {
		t.Fatal("resume renewed budget", remaining, err)
	}
	_, err = p.observeActivationBudget(ctx, d.State.ID, &resumed, now.Add(DefaultRecoveryBudget))
	if !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal(err)
	}
	if err := readProjectJSON(p.activationPath(d.State.ID), &resumed, 16384); err != nil || !resumed.Budget.Exhausted {
		t.Fatal("exhaustion not durable", err)
	}
}

func TestActivationExpiredResumeStopsBeforeNative(t *testing.T) {
	p, d, _, _ := projectFixture(t)
	if err := os.Remove(p.log(d.State.ID)); err != nil {
		t.Fatal(err)
	}
	a := activation{Schema: "lycheedev.channel-activation.v2", Request: "activation", Phase: "input_attempted", Budget: NewDurableBudget(time.Now().Add(-time.Hour), DefaultRecoveryBudget, false)}
	if err := writeProjectJSON(context.Background(), p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	r, err := p.resumeActivation(context.Background(), d.State.ID, false)
	if !errors.Is(err, ErrBudgetExhausted) || r.Continuation.Kind != "budget_exhausted" || r.Continuation.RemainingBudgetMS == nil || *r.Continuation.RemainingBudgetMS != 0 {
		t.Fatal(r, err)
	}
	if err := readProjectJSON(p.activationPath(d.State.ID), &a, 16384); err != nil || !a.Budget.Exhausted {
		t.Fatal("expiry not persisted", err)
	}
}

func TestActivationActiveDriverHasStructuredContinuation(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	a := activation{Schema: "lycheedev.channel-activation.v2", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(time.Now(), DefaultRecoveryBudget, false)}
	ctx := context.Background()
	if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(p.activationPath(d.State.ID))
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	r, err := p.resumeActivation(ctx, d.State.ID, false)
	var blocked *BlockedError
	if !errors.Is(err, ErrPending) || !errors.As(err, &blocked) || r.Continuation.Kind != "wait_active_driver" || r.Continuation.Blocker == nil || r.Continuation.Blocker.Consumer != d.State.ID {
		t.Fatal(r, err)
	}
	after, _ := os.ReadFile(p.activationPath(d.State.ID))
	if string(before) != string(after) {
		t.Fatal("mutated activation without driver lease")
	}
}

func TestActivationClockRollbackProjectionAndPersistence(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	now := time.Now()
	a := activation{Schema: "lycheedev.channel-activation.v2", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(now, DefaultRecoveryBudget, false)}
	r := p.presentActivation(d.State.ID, a, now.Add(-time.Second))
	if r.Continuation.Kind != "budget_exhausted" || a.Budget.Exhausted {
		t.Fatal("projection changed budget", r)
	}
	lease, err := journal.LockBootstrapWindow(context.Background(), parent, meta.Owner)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	_, err = p.observeActivationBudget(context.Background(), d.State.ID, &a, now.Add(-time.Second))
	if !errors.Is(err, ErrBudgetClockRollback) || !a.Budget.Exhausted {
		t.Fatal(err)
	}
	a.Budget = nil
	if a.validate() == nil {
		t.Fatal("v2 missing budget accepted")
	}
	a.Schema = "lycheedev.channel-activation.v3"
	if a.validate() == nil {
		t.Fatal("unknown schema accepted")
	}
}

func TestActivationMonotonicExpiryCannotRevive(t *testing.T) {
	for _, budgetTimeout := range []bool{false, true} {
		t.Run(map[bool]string{false: "caller", true: "budget"}[budgetTimeout], func(t *testing.T) {
			p, d, meta, parent := projectFixture(t)
			lease, err := journal.LockBootstrapWindow(context.Background(), parent, meta.Owner)
			if err != nil {
				t.Fatal(err)
			}
			defer lease.Close()
			a := activation{Schema: "lycheedev.channel-activation.v2", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(time.Now(), DefaultRecoveryBudget, false)}
			if err := writeProjectJSON(context.Background(), p.activationPath(d.State.ID), a); err != nil {
				t.Fatal(err)
			}
			cause := error(context.DeadlineExceeded)
			if budgetTimeout {
				cause = ErrBudgetExhausted
			}
			ctx, cancel := context.WithTimeoutCause(context.Background(), time.Nanosecond, cause)
			defer cancel()
			<-ctx.Done()
			err = p.finishActivationBudget(ctx, d.State.ID, &a)
			if budgetTimeout {
				if !errors.Is(err, ErrBudgetExhausted) || !a.Budget.Exhausted {
					t.Fatal("monotonic expiry revived", err)
				}
			} else if err != nil || a.Budget.Exhausted {
				t.Fatal("caller timeout exhausted goal", err)
			}
			var resumed activation
			if err := readProjectJSON(p.activationPath(d.State.ID), &resumed, 16384); err != nil {
				t.Fatal(err)
			}
			if resumed.Budget.Exhausted != budgetTimeout {
				t.Fatal("expiry not persisted")
			}
		})
	}
}
