package channel

import (
	"context"
	"encoding/json"
	"errors"
	"hash/adler32"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

func budgetDriverFixture(t *testing.T) (*Driver, *stopReconcilePeer, *time.Time) {
	t.Helper()
	d, peer := stopReconcileFixture(t)
	now := time.Unix(1800000000, 0)
	d.Now = func() time.Time { return now }
	d.State.ConnectBudget = NewDurableBudget(now, DefaultConnectBudget, false)
	return d, peer, &now
}

func budgetDriverContinue(d *Driver) error {
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Millisecond)
	defer cancel()
	return d.Continue(ctx)
}

func TestDriverBudgetResumeAndProgressKeepOriginalDeadline(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "durable-deadline", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	deadline := d.State.Operation.RecoveryBudget.DeadlineMS
	if err := budgetDriverContinue(d); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	progress := d.State.ProgressVersion
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	*now = now.Add(100 * time.Second)
	loaded.Now = func() time.Time { return *now }
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if loaded.State.Operation.RecoveryBudget.DeadlineMS != deadline {
		t.Fatal("resume renewed the deadline")
	}
	if loaded.State.ProgressVersion != progress {
		t.Fatal("repeated blocked wait counted as progress")
	}
	*now = time.UnixMilli(deadline)
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal("expired request continued", err)
	}
}

func TestDriverRepeatedCloseKeepsDeadlineAcrossLoad(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "prepared-close", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	d.State.Operation.Stage = "commit_ready"
	d.State.Operation.PreparedNonce = strings.Repeat("3", 32)
	d.State.Operation.Challenge = strings.Repeat("4", 32)
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	deadline := d.State.CloseBudget.DeadlineMS
	if err := budgetDriverContinue(d); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	*now = now.Add(100 * time.Second)
	loaded.Now = func() time.Time { return *now }
	progress := loaded.State.ProgressVersion
	if err := loaded.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if loaded.State.CloseBudget.DeadlineMS != deadline || loaded.State.ProgressVersion != progress {
		t.Fatal("repeat close renewed deadline or progress")
	}
	*now = time.UnixMilli(deadline)
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal(err)
	}
}

func TestDriverExpiredBusinessStillClosesUnpublishedWork(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "expired-business", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	*now = time.UnixMilli(d.State.Operation.RecoveryBudget.DeadlineMS)
	if err := budgetDriverContinue(d); !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal(err)
	}
	if len(peer.publications) != 0 || len(peer.inputs) != 0 {
		t.Fatal("expired business sent input")
	}
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	// The business deadline above uses the injected clock. Allow real journal
	// flushes to finish under disk contention; this is not a 350ms latency test.
	if err := stopReconcileContinueFor(d, 5*time.Second); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || d.State.Operation.Stage != "cancelled" {
		t.Fatalf("close did not retire unpublished work: %+v", d.State)
	}
	for _, e := range peer.publications {
		if e.Action != "unbind" {
			t.Fatal("close started business", e.Action)
		}
	}
}

func TestDriverLegacyJournalRejectedWithoutMigration(t *testing.T) {
	d, peer, _ := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "legacy-budget", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	d.State.Schema = "lycheedev.channel.v1"
	d.State.ConnectBudget, d.State.Operation.RecoveryBudget = nil, nil
	data, err := json.Marshal(d.State)
	if err != nil {
		t.Fatal(err)
	}
	if err := journal.AppendMemoryEvent(context.Background(), d.Log, "legacy_fixture", json.RawMessage(data)); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(d.Log)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Load(d.Log, peer); err == nil {
		t.Fatal("legacy native journal resumed")
	}
	after, err := os.ReadFile(d.Log)
	if err != nil || string(before) != string(after) {
		t.Fatal("legacy journal changed", err)
	}
	if len(peer.inputs) != 0 || len(peer.publications) != 0 {
		t.Fatal("legacy protocol produced side effects")
	}
}

func TestDriverCompletedEvidenceIgnoresExpiredRecoveryBudget(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "completed-budget", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage, op.PreparedNonce, op.Challenge = "complete", strings.Repeat("3", 32), strings.Repeat("4", 32)
	op.Result = []byte(`{}`)
	op.ReportBytes, op.ReportChecksum = uint32(len(op.Result)), adler32.Checksum(op.Result)
	if err := d.Save(context.Background(), "operation_complete"); err != nil {
		t.Fatal(err)
	}
	deadline := op.RecoveryBudget.DeadlineMS
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	*now = time.UnixMilli(deadline).Add(time.Hour)
	loaded.Now = func() time.Time { return *now }
	if err := budgetDriverContinue(loaded); err != nil {
		t.Fatal("completed evidence refused", err)
	}
	if loaded.State.Operation.RecoveryBudget.DeadlineMS != deadline || len(peer.publications) != 0 || len(peer.inputs) != 0 {
		t.Fatal("reading completed result renewed or executed work")
	}
	if loaded.continuation().Kind != "completed" {
		t.Fatal("completed evidence appears blocked")
	}
}

func TestDriverClockRollbackIsPersistedAndCannotRevive(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "rollback-budget", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	*now = now.Add(10 * time.Second)
	if err := budgetDriverContinue(d); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	*now = now.Add(-time.Second)
	if err := budgetDriverContinue(d); !errors.Is(err, ErrBudgetClockRollback) {
		t.Fatal("rollback permitted work", err)
	}
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	*now = now.Add(20 * time.Second)
	loaded.Now = func() time.Time { return *now }
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrBudgetClockRollback) {
		t.Fatal("restart forgot rollback", err)
	}
	if loaded.continuation().Kind != "budget_exhausted" {
		t.Fatal("rollback continuation not stopped")
	}
}

func TestDriverMonotonicBudgetExpiryPersistsExactBlocker(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "monotonic-expiry", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	// Keep wall time frozen: the invocation's monotonic deadline must still
	// exhaust the durable goal, including after the driver is reconstructed.
	d.State.Operation.RecoveryBudget = NewDurableBudget(*now, 100*time.Millisecond, false)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	err := d.Continue(ctx)
	cancel()
	if !errors.Is(err, ErrBudgetExhausted) || !errors.Is(err, ErrPending) {
		t.Fatal("monotonic expiry lost its budget cause", err)
	}
	assertStopped := func(driver *Driver) {
		t.Helper()
		b := driver.State.Blocker
		if !driver.State.Operation.RecoveryBudget.Exhausted || b == nil || b.Kind != "budget_exhausted" || b.Condition != "explicit_budget_decision" {
			t.Fatalf("incorrect exhaustion evidence: budget=%+v blocker=%+v", driver.State.Operation.RecoveryBudget, b)
		}
		if driver.continuation().Kind != "budget_exhausted" {
			t.Fatal("continuation did not stop exhausted goal")
		}
	}
	assertStopped(d)
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	loaded.Now = func() time.Time { return *now }
	assertStopped(loaded)
	publications, inputs := len(peer.publications), len(peer.inputs)
	if err := budgetDriverContinue(loaded); !errors.Is(err, ErrBudgetExhausted) {
		t.Fatal("restart revived monotonic-expired budget", err)
	}
	if len(peer.publications) != publications || len(peer.inputs) != inputs {
		t.Fatal("expired restart emitted additional work")
	}
}

func TestDriverCallerTimeoutDoesNotExhaustDurableBudget(t *testing.T) {
	d, peer, now := budgetDriverFixture(t)
	if err := d.PrepareRequest(context.Background(), "caller-timeout", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	d.State.Operation.RecoveryBudget = NewDurableBudget(*now, 100*time.Millisecond, false)
	if err := budgetDriverContinue(d); !errors.Is(err, ErrPending) || errors.Is(err, ErrBudgetExhausted) {
		t.Fatal("caller wait confused with durable deadline", err)
	}
	loaded, err := Load(d.Log, peer)
	if err != nil {
		t.Fatal(err)
	}
	loaded.Now = func() time.Time { return *now }
	if loaded.State.Operation.RecoveryBudget.Exhausted || loaded.State.Blocker != nil && loaded.State.Blocker.Kind == "budget_exhausted" {
		t.Fatal("shorter caller wait exhausted durable goal")
	}
	if loaded.continuation().Kind != "continue" {
		t.Fatalf("shorter wait prevented continuation: %+v", loaded.continuation())
	}
}
