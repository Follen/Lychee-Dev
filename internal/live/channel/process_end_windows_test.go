//go:build windows && amd64

package channel

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"os"
	"testing"
)

func TestDisconnectEndedLifetimeRetiresOnlyExactOwnerAndIsIdempotent(t *testing.T) {
	p, d, meta, parent := projectFixture(t)
	ctx := context.Background()
	// An impossible creation counter for this real PID models PID reuse. No
	// window or game input is available to this test, so a native drive must fail.
	if err := journal.RetireConnectionWindow(ctx, parent, meta.Owner); err != nil {
		t.Fatal(err)
	}
	meta.Target.Window = desktop.WindowIdentity{ProcessID: uint32(os.Getpid()), ProcessStartedAt: 1, Handle: 789}
	meta.Owner.Resource = fmt.Sprintf("window/%d/1/789", os.Getpid())
	if err := journal.BeginConnectionWindow(ctx, parent, meta.Owner, func() error { return writeProjectJSON(ctx, p.path("connections", d.State.ID+".target.json"), meta) }); err != nil {
		t.Fatal(err)
	}
	if err := d.PrepareRequest(ctx, "interrupted", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	f, err := os.OpenFile(d.Log, os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = f.WriteString(`{"sequence":`); err != nil {
		t.Fatal(err)
	}
	if err = f.Close(); err != nil {
		t.Fatal(err)
	}
	for n := 0; n < 2; n++ {
		r, err := p.Disconnect(ctx, d.State.ID, false)
		if err != nil || !r.Closed || r.Complete || r.ReportState != "unavailable" || r.ProcessEnd != "pid_reused" {
			t.Fatalf("%+v %v", r, err)
		}
	}
	if _, busy, err := journal.InspectWindowOwner(ctx, parent, meta.Owner.Resource); err != nil || busy {
		t.Fatal("claim retained", err)
	}
	events, err := journal.ReadMemoryLog(d.Log)
	if err != nil {
		t.Fatal(err)
	}
	// Intent plus terminal evidence remain; repeated disconnect appends nothing.
	if len(events) < 4 {
		t.Fatal("missing durable process-end evidence")
	}
	before := len(events)
	if _, err = p.Disconnect(ctx, d.State.ID, false); err != nil {
		t.Fatal(err)
	}
	events, err = journal.ReadMemoryLog(d.Log)
	if err != nil || len(events) != before {
		t.Fatal("non-idempotent retry", err)
	}
}

func TestProcessEndNeverPromotesUnconfirmedResult(t *testing.T) {
	for _, stage := range []string{"prepared", "commit_ready", "running", "confirm_ready", "execution_unknown", "result_verified", "release_ready", "complete"} {
		t.Run(stage, func(t *testing.T) {
			s := State{Bound: true, Closing: true, Transaction: &Transaction{}, Reload: &ReloadAttempt{Phase: "input_attempted"}, Operation: &Operation{Stage: stage, Result: []byte(`{"ok":true}`)}}
			closeEndedState(&s)
			verified := stage == "result_verified" || stage == "release_ready" || stage == "complete"
			if (s.Operation.Stage == "complete") != verified {
				t.Fatal("changed report authority", s.Operation)
			}
			if s.Bound || !s.Closed || s.Closing || s.Transaction != nil || s.Reload != nil {
				t.Fatal("not closed")
			}
			if !verified && s.Operation.Stage != "execution_unknown" {
				t.Fatal("unknown result lost")
			}
			if len(s.Operation.Result) == 0 {
				t.Fatal("candidate evidence discarded")
			}
		})
	}
}
