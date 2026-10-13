package channel

import (
	"bytes"
	"context"
	"errors"
	"hash/adler32"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type lostReleasePeer struct {
	*stopReconcilePeer
	released bool
	blocked  bool
}

func (p *lostReleasePeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	if q.Envelope.Action == "release" && p.released {
		e := q.Envelope
		i := p.current
		i.Schema, i.Owner, i.Fence, i.NextSlot = e.Schema, e.Owner, e.Fence, e.Index+1
		return Observation{Receipt: Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action, State: "released"}}, nil
	}
	return p.stopReconcilePeer.Observe(ctx, q)
}

func (p *lostReleasePeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if p.blocked && e.Action == "reload" {
		return InputObservation{}, ErrInputObservationStale
	}
	return p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
}

func lostReleaseFixture(t *testing.T) (*Driver, *lostReleasePeer, *time.Time) {
	t.Helper()
	d, transport := stopReconcileFixture(t)
	p := &lostReleasePeer{stopReconcilePeer: transport}
	d.Backend = p
	now := time.Now()
	d.Now = func() time.Time { return now }
	ctx := context.Background()
	if err := d.PrepareRequest(ctx, "verified-release", "return 1", 5, "opaque"); err != nil {
		t.Fatal(err)
	}
	op := d.State.Operation
	op.Stage, op.PreparedNonce, op.Challenge = "release_ready", strings.Repeat("3", 32), strings.Repeat("4", 32)
	op.Result = []byte(`{"ok":true,"resourcesReleased":true,"result":21}`)
	op.ReportBytes, op.ReportChecksum = uint32(len(op.Result)), adler32.Checksum(op.Result)
	if err := d.begin(ctx, "release", op.Ticket, func(e *bridge.SlotEnvelope) {
		e.ReportBytes, e.ReportChecksum = op.ReportBytes, op.ReportChecksum
	}); err != nil {
		t.Fatal(err)
	}
	d.State.Transaction.Phase = "published"
	if err := d.invoke(ctx, d.State.Transaction); err != nil {
		t.Fatal(err)
	}
	return d, p, &now
}

func TestMissingReleaseReceiptRecoversVerifiedResultWithoutReplay(t *testing.T) {
	d, p, now := lostReleaseFixture(t)
	ctx := context.Background()
	op := *d.State.Operation
	tx := d.State.Transaction.Envelope
	deadline := op.RecoveryBudget.DeadlineMS
	if err := d.next(ctx); !errors.Is(err, ErrPending) || d.State.Reload != nil {
		t.Fatal("must first wait for the original receipt", err)
	}
	*now = now.Add(6 * time.Second)
	if err := d.next(ctx); !errors.Is(err, errProgress) || d.State.Reload == nil {
		t.Fatal("verified report remained stuck after missing release callback", err)
	}
	if !d.State.Reload.Automatic || !d.State.Reload.ReconcilePending || d.State.Reload.RecoveryBudget.DeadlineMS != deadline || d.State.Transaction.Envelope != tx {
		t.Fatal("recovery lost original transaction or extended deadline")
	}
	// Restart after the durable recovery decision. No old release is resent.
	var err error
	d, err = Load(d.Log, p)
	if err != nil {
		t.Fatal(err)
	}
	p.blocked = true
	if err := d.next(ctx); !errors.Is(err, ErrInputObservationStale) || len(p.inputs) != 1 {
		t.Fatal("reload bypassed fresh INPUT", err)
	}
	p.blocked = false
	if err := d.next(ctx); !errors.Is(err, errProgress) || len(p.inputs) != 2 || p.inputs[1].Kind != "reload" {
		t.Fatal("missing single recovery reload", err, p.inputs)
	}
	if err := d.next(ctx); !errors.Is(err, ErrPending) || len(p.inputs) != 2 {
		t.Fatal("uncertain reload was repeated", err)
	}
	p.current.Runtime, p.current.NextSlot = strings.Repeat("2", 32), 1
	if err := stopReconcileContinue(d); err != nil {
		t.Fatal(err)
	}
	got := d.State.Operation
	if got.Stage != "complete" || got.CleanupMethod != "runtime_destroyed" || got.ID != op.ID || got.Ticket != op.Ticket || got.Attempt != op.Attempt || !bytes.Equal(got.Result, op.Result) || got.RecoveryBudget.DeadlineMS != deadline {
		t.Fatal("recovery did not retain the exact verified report", got)
	}
	if len(p.inputs) != 3 || p.inputs[0].Envelope.Action != "release" || p.inputs[2].Envelope.Action != "bind" || len(p.publications) != 1 || p.publications[0].Action != "bind" {
		t.Fatal("replayed business/release", p.inputs, p.publications)
	}
}

func TestLateReleaseReceiptAvoidsRecoveryReload(t *testing.T) {
	d, p, now := lostReleaseFixture(t)
	if err := d.next(context.Background()); !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	*now = now.Add(6 * time.Second)
	p.released = true
	if err := d.next(context.Background()); err != nil {
		t.Fatal(err)
	}
	if d.State.Reload != nil || d.State.Operation.CleanupMethod != "released" || len(p.inputs) != 1 {
		t.Fatal("late exact receipt caused unnecessary reload")
	}
}
