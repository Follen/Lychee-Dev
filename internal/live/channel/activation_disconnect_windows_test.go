//go:build windows && amd64

package channel

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"golang.org/x/sys/windows"
)

func TestDisconnectUnsentActivationIsDurableAndIdempotent(t *testing.T) {
	for _, outcome := range []*InputOutcome{nil, {Disposition: "not_sent", Reason: "slot_pool_invalid"}} {
		p, d, meta, parent := projectFixture(t)
		ctx := context.Background()
		// Use this process's actual lifetime so the old Disconnect reaches its
		// ordinary driver and fails with ErrJournalMissing, as on the real client.
		var created, exited, kernel, user windows.Filetime
		if err := windows.GetProcessTimes(windows.CurrentProcess(), &created, &exited, &kernel, &user); err != nil {
			t.Fatal(err)
		}
		if err := journal.RetireConnectionWindow(ctx, parent, meta.Owner); err != nil {
			t.Fatal(err)
		}
		meta.Target.Window.ProcessID = uint32(os.Getpid())
		meta.Target.Window.ProcessStartedAt = uint64(created.HighDateTime)<<32 | uint64(created.LowDateTime)
		meta.Owner.Resource = fmt.Sprintf("window/%d/%d/0", meta.Target.Window.ProcessID, meta.Target.Window.ProcessStartedAt)
		if err := journal.BeginConnectionWindow(ctx, parent, meta.Owner, func() error {
			return writeProjectJSON(ctx, p.path("connections", d.State.ID+".target.json"), meta)
		}); err != nil {
			t.Fatal(err)
		}
		if err := os.Remove(d.Log); err != nil {
			t.Fatal(err)
		}
		a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "prepared", Outcome: outcome, Budget: NewDurableBudget(time.Now().Add(-time.Hour), DefaultRecoveryBudget, false)}
		if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
			t.Fatal(err)
		}
		for _, call := range []func() (ProjectResult, error){
			func() (ProjectResult, error) { return p.Disconnect(ctx, d.State.ID, false) },
			func() (ProjectResult, error) { return p.Status(d.State.ID) },
			func() (ProjectResult, error) { return p.Resume(ctx, d.State.ID, false) },
			func() (ProjectResult, error) { return p.Disconnect(ctx, d.State.ID, false) },
		} {
			r, err := call()
			if err != nil || !r.Closed || r.Complete || r.Stage != "activation_cancelled" || r.ReportState != "unavailable" || r.Continuation.Kind != "completed" {
				t.Fatalf("%+v %v", r, err)
			}
		}
		if _, err := os.Stat(d.Log); !os.IsNotExist(err) {
			t.Fatal("created business ledger", err)
		}
		if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
			t.Fatal("claim retained", err)
		}
	}
}

func TestDisconnectActivationRefusesUncertainStateAndActiveDriver(t *testing.T) {
	for _, kind := range []string{"input_attempted", "runtime_selected", "nonzero_step", "uncertain", "submitted", "invalid_not_sent", "invalid_cancelled", "active_driver", "foreign_owner"} {
		t.Run(kind, func(t *testing.T) {
			p, d, meta, parent := projectFixture(t)
			ctx := context.Background()
			if err := os.Remove(d.Log); err != nil {
				t.Fatal(err)
			}
			a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "prepared", Budget: NewDurableBudget(time.Now(), DefaultRecoveryBudget, false)}
			switch kind {
			case "input_attempted", "runtime_selected":
				a.Phase = kind
			case "nonzero_step":
				a.InputStep = 1
			case "uncertain":
				a.Outcome = &InputOutcome{Disposition: "uncertain"}
			case "submitted":
				a.Outcome = &InputOutcome{Disposition: "submitted", MessagesQueued: 1}
			case "invalid_not_sent":
				a.Outcome = &InputOutcome{Disposition: "not_sent", MessagesQueued: 1}
			case "invalid_cancelled":
				a.Phase, a.InputStep = "cancelled", 1
			case "active_driver":
				lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
				if err != nil {
					t.Fatal(err)
				}
				defer lease.Close()
			case "foreign_owner":
				if err := journal.RetireConnectionWindow(ctx, parent, meta.Owner); err != nil {
					t.Fatal(err)
				}
				meta.Owner.OperationID = "CON-ffffffffffffffffffffffffffffffff"
				if err := journal.BeginConnectionWindow(ctx, parent, meta.Owner, func() error { return nil }); err != nil {
					t.Fatal(err)
				}
			}
			if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
				t.Fatal(err)
			}
			before, _ := os.ReadFile(p.activationPath(d.State.ID))
			if r, err := p.Disconnect(ctx, d.State.ID, false); err == nil || r.Closed {
				t.Fatalf("unsafe cancellation: %+v %v", r, err)
			}
			after, _ := os.ReadFile(p.activationPath(d.State.ID))
			if string(before) != string(after) {
				t.Fatal("activation changed")
			}
			if owner, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || !busy || owner != meta.Owner {
				t.Fatal("owner changed", err)
			}
		})
	}
}

func TestCancelledActivationRecoversRetirementWithoutTouchingNewOwner(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	ctx := context.Background()
	if err := os.Remove(d.Log); err != nil {
		t.Fatal(err)
	}
	a := activation{Schema: "lycheedev.channel-activation.v3", Request: "activation", Phase: "cancelled", Budget: NewDurableBudget(time.Now().Add(-time.Hour), DefaultRecoveryBudget, false)}
	if err := writeProjectJSON(ctx, p.activationPath(d.State.ID), a); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(p.activationPath(d.State.ID))
	// Models a crash after terminal persistence but before retiring the claim.
	if r, err := p.Resume(ctx, d.State.ID, false); err != nil || !r.Closed {
		t.Fatal(r, err)
	}
	if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
		t.Fatal("claim retained", err)
	}
	meta.Owner.OperationID = "CON-ffffffffffffffffffffffffffffffff"
	if err := journal.BeginConnectionWindow(ctx, parent, meta.Owner, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	if r, err := p.Disconnect(ctx, d.State.ID, false); err != nil || !r.Closed {
		t.Fatal(r, err)
	}
	if r, err := p.Resume(ctx, d.State.ID, false); err != nil || !r.Closed {
		t.Fatal(r, err)
	}
	if owner, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || !busy || owner != meta.Owner {
		t.Fatal("new owner changed", err)
	}
	after, _ := os.ReadFile(p.activationPath(d.State.ID))
	if string(before) != string(after) {
		t.Fatal("terminal record changed")
	}
}
