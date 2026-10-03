//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"fmt"
	"os"
	"reflect"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

func pendingActivationFixture(t *testing.T, outcome *InputOutcome) (*Project, string, projectTarget, string, activation) {
	t.Helper()
	p, d, meta, parent := projectFixture(t)
	ctx := context.Background()
	if err := os.Remove(d.Log); err != nil {
		t.Fatal(err)
	}
	// A reused PID reaches the old disconnect path without any game process.
	if err := journal.RetireConnectionWindow(ctx, parent, meta.Owner); err != nil {
		t.Fatal(err)
	}
	meta.Target.Window = desktop.WindowIdentity{ProcessID: uint32(os.Getpid()), ProcessStartedAt: 1, Handle: 789}
	meta.Owner.Resource = fmt.Sprintf("window/%d/1/789", os.Getpid())
	a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "input_attempted", Outcome: outcome, Budget: NewDurableBudget(time.Now().Add(-time.Hour), DefaultRecoveryBudget, false)}
	if outcome != nil && outcome.Disposition == "not_sent" {
		a.Phase = "prepared"
	}
	if err := journal.BeginConnectionWindow(ctx, parent, meta.Owner, func() error {
		if err := writeProjectJSON(ctx, p.path("connections", d.State.ID+".target.json"), meta); err != nil {
			return err
		}
		return writeProjectJSON(ctx, p.activationPath(d.State.ID), a)
	}); err != nil {
		t.Fatal(err)
	}
	return p, d.State.ID, meta, parent, a
}

func TestDisconnectActivationDoesNotInspectLiveProcess(t *testing.T) {
	p, id, meta, parent, _ := pendingActivationFixture(t, nil)
	// No valid OS process identity, window, managed installation or memory is
	// available. Host-only cancellation must not depend on any of them.
	meta.Target.Window = desktop.WindowIdentity{}
	if err := writeProjectJSON(context.Background(), p.path("connections", id+".target.json"), meta); err != nil {
		t.Fatal(err)
	}
	if r, err := p.Disconnect(context.Background(), id, false); err != nil || !r.Closed {
		t.Fatal("attempted native process recovery", r, err)
	}
	if _, busy, err := journal.InspectWindowOwner(context.Background(), parent, meta.Owner.Resource); err != nil || busy {
		t.Fatal("claim retained", busy, err)
	}
}

func TestDisconnectActivationActiveDriverRetainsEvidence(t *testing.T) {
	p, id, meta, parent, _ := pendingActivationFixture(t, nil)
	ctx := context.Background()
	before, _ := os.ReadFile(p.activationPath(id))
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	r, err := p.Disconnect(ctx, id, false)
	if !errors.Is(err, ErrPending) || r.Closed || r.Continuation.Kind != "wait_active_driver" || r.Continuation.Goal != "close" || r.Continuation.Blocker == nil || r.Continuation.Blocker.Consumer != id {
		t.Fatal("did not wait for activation driver", r, err)
	}
	after, _ := os.ReadFile(p.activationPath(id))
	if string(before) != string(after) {
		t.Fatal("changed activation while another driver may send input")
	}
	if owner, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || !busy || owner != meta.Owner {
		t.Fatal("active driver's claim was changed", owner, busy, err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	if r, err := p.Disconnect(ctx, id, false); err != nil || !r.Closed {
		t.Fatal("could not cancel after active driver exit", r, err)
	}
}

func TestDisconnectActivationRepairsCrashAfterTerminalSave(t *testing.T) {
	for _, action := range []string{"disconnect", "resume", "activation_resume"} {
		t.Run(action, func(t *testing.T) {
			p, id, meta, parent, a := pendingActivationFixture(t, nil)
			a.AbandonedFrom, a.Phase = a.Phase, "abandoned"
			ctx := context.Background()
			if err := writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
				t.Fatal(err)
			}
			before, _ := os.ReadFile(p.activationPath(id))
			var r ProjectResult
			var err error
			switch action {
			case "disconnect":
				r, err = p.Disconnect(ctx, id, false)
			case "resume":
				r, err = p.Resume(ctx, id, false)
			case "activation_resume":
				r, err = p.resumeActivation(ctx, id, false)
			}
			if err != nil || !r.Closed || r.Complete || r.Continuation.Kind != "completed" || r.Continuation.Goal != "close" || r.Continuation.Blocker != nil {
				t.Fatal("terminal activation stuck on old budget", r, err)
			}
			if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
				t.Fatal("claim retained after terminal crash", busy, err)
			}
			after, _ := os.ReadFile(p.activationPath(id))
			if string(before) != string(after) {
				t.Fatal("crash recovery rewrote terminal evidence")
			}
		})
	}
}

func TestDisconnectActivationRefusesDamagedEvidence(t *testing.T) {
	for _, damage := range []string{"selected_runtime_missing_journal", "empty_journal", "corrupt_journal", "invalid_activation", "invalid_abandonment", "cancelled_context"} {
		t.Run(damage, func(t *testing.T) {
			p, id, meta, parent, a := pendingActivationFixture(t, nil)
			ctx := context.Background()
			switch damage {
			case "selected_runtime_missing_journal":
				a.Phase = "runtime_selected"
				if err := writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
					t.Fatal(err)
				}
			case "empty_journal", "corrupt_journal":
				var b []byte
				if damage == "corrupt_journal" {
					b = []byte("{}\n")
				}
				if err := os.WriteFile(p.log(id), b, 0600); err != nil {
					t.Fatal(err)
				}
			case "invalid_activation", "invalid_abandonment":
				if damage == "invalid_activation" {
					a.Schema = "invalid"
				} else {
					a.Phase, a.AbandonedFrom = "abandoned", "runtime_selected"
				}
				if err := writeProjectJSON(ctx, p.activationPath(id), a); err != nil {
					t.Fatal(err)
				}
			case "cancelled_context":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}
			before, _ := os.ReadFile(p.activationPath(id))
			if r, err := p.Disconnect(ctx, id, false); err == nil || r.Closed {
				t.Fatal("unsafe cancellation accepted", r, err)
			}
			after, _ := os.ReadFile(p.activationPath(id))
			if string(before) != string(after) {
				t.Fatal("damaged evidence changed")
			}
			if owner, busy, err := journal.InspectWindowOwner(context.Background(), parent, meta.Owner.Resource); err != nil || !busy || owner != meta.Owner {
				t.Fatal("unsafe cancellation released claim", owner, busy, err)
			}
		})
	}
}

func TestDisconnectActivationCannotReleaseForeignClaim(t *testing.T) {
	p, id, meta, parent, _ := pendingActivationFixture(t, nil)
	ctx := context.Background()
	if err := journal.RetireConnectionWindow(ctx, parent, meta.Owner); err != nil {
		t.Fatal(err)
	}
	foreign := meta.Owner
	foreign.OperationID = "CON-33333333333333333333333333333333"
	foreign.WorkspaceID = "44444444444444444444444444444444"
	if err := journal.BeginConnectionWindow(ctx, parent, foreign, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(p.activationPath(id))
	if r, err := p.Disconnect(ctx, id, false); err == nil || r.Closed {
		t.Fatal("foreign claim accepted", r, err)
	}
	after, _ := os.ReadFile(p.activationPath(id))
	if string(before) != string(after) {
		t.Fatal("unowned activation changed")
	}
	if owner, busy, err := journal.InspectWindowOwner(ctx, parent, foreign.Resource); err != nil || !busy || owner != foreign {
		t.Fatal("foreign claim released", owner, busy, err)
	}
}

func TestDisconnectActivationHandoffUsesOrdinaryJournal(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	ctx := context.Background()
	a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(time.Now().Add(-time.Hour), DefaultRecoveryBudget, false)}
	if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	d.State.Bound, d.State.Closed = false, true
	if err := d.Save(ctx, "fixture_closed"); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(p.activationPath(d.State.ID))
	// Directly enter the activation helper to model a normal journal appearing
	// between the caller's initial missing-log read and its lease acquisition.
	r, err := p.disconnectActivation(ctx, d.State.ID, false)
	if err != nil || !r.Closed || r.Cleanup == "abandoned" || r.Journal != d.Log {
		t.Fatal("ordinary connection was abandoned", r, err)
	}
	after, _ := os.ReadFile(p.activationPath(d.State.ID))
	if string(before) != string(after) {
		t.Fatal("handoff rewrote activation evidence")
	}
	if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
		t.Fatal("ordinary closed claim retained", busy, err)
	}
}

func TestDisconnectPendingActivationReleasesClaimWithoutRuntime(t *testing.T) {
	for _, disposition := range []string{"not_sent", "submitted", "uncertain", "missing_outcome"} {
		t.Run(disposition, func(t *testing.T) {
			var outcome *InputOutcome
			if disposition != "missing_outcome" {
				outcome = &InputOutcome{Disposition: disposition}
				if disposition != "not_sent" {
					outcome.MessagesQueued = 17
				}
			}
			p, id, meta, parent, original := pendingActivationFixture(t, outcome)
			ctx := context.Background()
			r, err := p.Disconnect(ctx, id, false)
			if err != nil || !r.Closed || r.Complete || r.Bound || r.Identity.Runtime != "" || r.ReportState != "unavailable" || r.Cleanup != "abandoned" {
				t.Fatalf("pending activation could not release its claim: %+v %v", r, err)
			}
			if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
				t.Fatal("activation claim retained", busy, err)
			}
			var saved activation
			if err := readProjectJSON(p.activationPath(id), &saved, 16384); err != nil {
				t.Fatal(err)
			}
			if saved.Phase != "abandoned" || saved.AbandonedFrom != original.Phase || !reflect.DeepEqual(saved.Outcome, original.Outcome) || !reflect.DeepEqual(saved.Budget, original.Budget) {
				t.Fatal("close changed original input evidence or budget", saved)
			}
			before, _ := os.ReadFile(p.activationPath(id))
			for _, action := range []func() (ProjectResult, error){
				func() (ProjectResult, error) { return p.Status(id) },
				func() (ProjectResult, error) { return p.Disconnect(ctx, id, false) },
				func() (ProjectResult, error) { return p.Resume(ctx, id, false) },
				func() (ProjectResult, error) { return p.resumeActivation(ctx, id, false) },
			} {
				if result, err := action(); err != nil || !result.Closed || result.Complete || result.Cleanup != "abandoned" {
					t.Fatal("terminal activation was reopened", result, err)
				}
			}
			after, _ := os.ReadFile(p.activationPath(id))
			if string(before) != string(after) {
				t.Fatal("terminal retry changed activation evidence")
			}
			// The same window can now be reserved by a fresh connection.
			fresh := meta.Owner
			fresh.OperationID = "CON-22222222222222222222222222222222"
			if err := journal.BeginConnectionWindow(ctx, parent, fresh, func() error { return nil }); err != nil {
				t.Fatal("next connection still blocked", err)
			}
			if _, err := p.Disconnect(ctx, id, false); err != nil {
				t.Fatal("terminal retry failed after new owner", err)
			}
			if owner, busy, err := journal.InspectWindowOwner(ctx, parent, fresh.Resource); err != nil || !busy || owner != fresh {
				t.Fatal("terminal retry touched new owner", owner, busy, err)
			}
		})
	}
}
