package channel

import (
	"context"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Reproduce both input-read boundaries and runtime discovery using the same
// original transaction. Cancellation on the second observation makes retries
// deterministic without sending another input or renewing the durable budget.
type mailboxWaitPeer struct {
	*stopReconcilePeer
	point  string
	fault  error
	reads  int
	cancel context.CancelFunc
}

func (p *mailboxWaitPeer) Publish(_ context.Context, e bridge.SlotEnvelope) error {
	p.publications = append(p.publications, e)
	return nil
}
func (p *mailboxWaitPeer) observation() error {
	p.reads++
	if p.reads == 1 {
		return p.fault
	}
	p.cancel()
	return ErrPending
}
func (p *mailboxWaitPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	if p.point == "before_input" {
		return InputObservation{}, p.observation()
	}
	return p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
}
func (p *mailboxWaitPeer) Observe(_ context.Context, _ ObservationQuery) (Observation, error) {
	if p.point == "receipt" {
		return Observation{}, p.observation()
	}
	return Observation{}, ErrPending
}
func (p *mailboxWaitPeer) Input(ctx context.Context, a InputAction) (InputOutcome, error) {
	if p.point == "input_recheck" {
		err := p.observation()
		return InputOutcome{Disposition: "not_sent", Reason: err.Error()}, err
	}
	return p.stopReconcilePeer.Input(ctx, a)
}
func (p *mailboxWaitPeer) RuntimeCandidate(_ context.Context, _ Identity) (*Identity, error) {
	if p.point == "reload" {
		return nil, p.observation()
	}
	return nil, nil
}

func TestMailboxTransientObservationContinuesOriginalGoal(t *testing.T) {
	for _, tc := range []struct {
		point string
		fault error
		sent  int
	}{
		{"before_input", memory.ErrMailboxPathChanged, 0},
		{"input_recheck", memory.ErrMailboxPathChanged, 0},
		{"receipt", memory.ErrMailboxPathChanged, 1},
		{"reload", memory.ErrMailboxStateNull, 1},
		{"reload", memory.ErrMailboxStringLayout, 1},
	} {
		t.Run(tc.point+"/"+tc.fault.Error(), func(t *testing.T) {
			d, base := stopReconcileFixture(t)
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			p := &mailboxWaitPeer{stopReconcilePeer: base, point: tc.point, fault: tc.fault, cancel: cancel}
			d.Backend = p
			var originalBudget DurableBudget
			if tc.point == "reload" {
				if err := d.RequestReload(ctx, "original-reload"); err != nil {
					t.Fatal(err)
				}
				originalBudget = *d.State.Reload.RecoveryBudget
			} else {
				if err := d.PrepareRequest(ctx, "original-operation", "return 1", 10, "observation"); err != nil {
					t.Fatal(err)
				}
				originalBudget = *d.State.Operation.RecoveryBudget
			}
			err := d.Continue(ctx)
			if !errors.Is(err, ErrPending) || p.reads != 2 || len(p.inputs) != tc.sent {
				t.Fatalf("temporary read ended the goal or replayed input: err=%v reads=%d inputs=%d", err, p.reads, len(p.inputs))
			}
			ref, _, _ := d.goalBudget()
			if ref == nil || *ref == nil || (*ref).StartedAtMS != originalBudget.StartedAtMS || (*ref).DeadlineMS != originalBudget.DeadlineMS {
				t.Fatal("temporary read changed the original recovery deadline")
			}
			if tc.point != "reload" && len(p.publications) != 1 {
				t.Fatal("republished original slot")
			}
		})
	}
}

func TestMailboxPermanentReadFailureStillStops(t *testing.T) {
	for _, tc := range []struct {
		point string
		fault error
	}{
		{"before_input", memory.ErrMailboxStringLayout},
		{"before_input", memory.ErrMailboxStateNull},
		{"reload", memory.ErrMailboxIncompatible},
		{"reload", fmt.Errorf("%w: root_recipe_changed", memory.ErrMailboxUnavailable)},
		{"reload", errors.New("memory.process_changed")},
	} {
		t.Run(tc.point+"/"+tc.fault.Error(), func(t *testing.T) {
			d, base := stopReconcileFixture(t)
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			p := &mailboxWaitPeer{stopReconcilePeer: base, point: tc.point, fault: tc.fault, cancel: cancel}
			d.Backend = p
			if tc.point == "reload" {
				if err := d.RequestReload(ctx, "read-failure"); err != nil {
					t.Fatal(err)
				}
			} else if err := d.PrepareRequest(ctx, "read-failure", "return 1", 10, "observation"); err != nil {
				t.Fatal(err)
			}
			err := d.Continue(ctx)
			if !errors.Is(err, tc.fault) || errors.Is(err, ErrPending) || p.reads != 1 {
				t.Fatalf("permanent failure was swallowed: %v reads=%d", err, p.reads)
			}
		})
	}
}
