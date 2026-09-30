//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Model the actual Native seam used by the optional retirement probe without
// waiting thirty seconds or touching a game/window. The probe's own deadline
// remains short; its synthetic timeout permits ordinary unbind to continue.
type retirementScopePeer struct {
	*stopReconcilePeer
	native        Native
	source        observationSource
	wantDeadline  time.Time
	probeDeadline time.Time
	globalAtProbe time.Time
	observed      int
}

func (p *retirementScopePeer) beginObservation(ctx context.Context, identity Identity) error {
	return p.native.beginObservation(ctx, identity)
}

func (p *retirementScopePeer) ObserveRuntimeReplacement(ctx context.Context, _ Identity) (*RuntimeReplacementProof, error) {
	p.observed++
	bounded, cancel := p.native.observationContext(ctx)
	defer cancel()
	p.probeDeadline, _ = bounded.Deadline()
	p.globalAtProbe = p.native.observation.deadline
	if _, err := p.native.memorySource(&p.source).Read(bounded, 0, make([]byte, 1)); err != nil {
		return nil, err
	}
	return nil, context.DeadlineExceeded
}

func (p *retirementScopePeer) RetireReplacedRuntime(context.Context, bridge.SlotEnvelope, *RuntimeReplacementProof) error {
	return errors.New("no replacement proof in scope fixture")
}

func (p *retirementScopePeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	bounded, cancel := p.native.observationContext(ctx)
	defer cancel()
	deadline, _ := bounded.Deadline()
	// Continue may tighten by the milliseconds spent persisting its durable
	// budget, but an optional thirty-second deadline must never become global.
	if deadline.Before(p.wantDeadline.Add(-time.Second)) {
		return InputObservation{}, errors.New("optional_probe_poisoned_observation_deadline")
	}
	return p.stopReconcilePeer.ObserveInput(bounded, e, after, capability)
}

func TestRetirementProbePreservesOuterObservationScope(t *testing.T) {
	for _, existing := range []bool{false, true} {
		name := "fresh_native"
		if existing {
			name = "existing_deadline_and_credit"
		}
		t.Run(name, func(t *testing.T) {
			d, transport := stopReconcileFixture(t)
			if err := d.RequestClose(context.Background()); err != nil {
				t.Fatal(err)
			}
			budget := *d.State.CloseBudget
			outer, cancel := context.WithTimeout(context.Background(), 120*time.Second)
			defer cancel()
			outerDeadline, _ := outer.Deadline()
			peer := &retirementScopePeer{stopReconcilePeer: transport, source: observationSource{data: []byte{1}}, wantDeadline: outerDeadline}
			priorReads := uint64(0)
			if existing {
				prior, stop := context.WithTimeout(outer, 45*time.Second)
				defer stop()
				if err := peer.native.beginObservation(prior, d.State.Identity); err != nil {
					t.Fatal(err)
				}
				peer.native.observation.session = memory.NewSession(memory.Budget{MaxReadCalls: 2})
				peer.wantDeadline, _ = prior.Deadline()
				if _, err := peer.native.memorySource(&peer.source).Read(prior, 0, make([]byte, 1)); err != nil {
					t.Fatal(err)
				}
				priorReads = 1
			} else if peer.native.observation != nil {
				t.Fatal("fixture is not a fresh Native")
			}
			d.Backend = peer
			if err := d.continueOrRetireWith(outer, peer, retirementTarget(), true); err != nil || !d.State.Closed {
				t.Fatalf("optional probe curtailed ordinary close: closed=%t err=%v global=%v probe=%v", d.State.Closed, err, peer.native.observation.deadline, peer.probeDeadline)
			}
			if peer.observed != 1 || !peer.globalAtProbe.Equal(peer.wantDeadline) || !peer.probeDeadline.Before(peer.globalAtProbe.Add(-time.Second)) {
				t.Fatalf("nested probe changed global scope: global=%v want=%v probe=%v calls=%d", peer.globalAtProbe, peer.wantDeadline, peer.probeDeadline, peer.observed)
			}
			if st := peer.native.memorySession().Stats(); st.ReadCalls != priorReads+1 {
				t.Fatalf("probe reset aggregate credit: %+v", st)
			}
			if d.State.CloseBudget.StartedAtMS != budget.StartedAtMS || d.State.CloseBudget.DeadlineMS != budget.DeadlineMS {
				t.Fatal("scope initialization renewed durable close budget")
			}
			if len(transport.inputs) != 1 || transport.inputs[0].Kind != "invoke" || len(transport.publications) != 1 || transport.publications[0].Action != "unbind" {
				t.Fatal("ordinary unbind did not execute exactly once")
			}
			if existing {
				if _, err := peer.native.memorySource(&peer.source).Read(outer, 0, make([]byte, 1)); !errors.Is(err, memory.ErrBudget) {
					t.Fatal("existing aggregate budget was replenished", err)
				}
			}
		})
	}
}
