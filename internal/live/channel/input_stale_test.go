package channel

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestInputPostLookupExpiryIsNarrow(t *testing.T) {
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, Index: 1, GUID: "g", Build: "b"}
	blocked := false
	s := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: 1, GUID: e.GUID, Build: e.Build, SampleMillis: 1000, InputBlocked: &blocked}
	encode := func(s InputObservation) memory.Record {
		return inputTestRecord(t, s)
	}
	r := encode(s)
	if _, err := inputObservation(r, e, 0, 1100); err != nil {
		t.Fatal("selector should accept", err)
	}
	if got, err := inputAfterLookup([]memory.Record{r}, e, 0, 1500); err != nil || got.Address != 123 {
		t.Fatal("500ms boundary changed", err)
	}
	if _, err := inputAfterLookup([]memory.Record{r}, e, 0, 1501); !errors.Is(err, ErrInputObservationStale) || !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if _, err := inputObservation(r, e, 0, 1501); err == nil {
		t.Fatal("old heap record accepted next lookup")
	}
	if _, err := inputAfterLookup(nil, e, 0, 1501); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) {
		t.Fatal("missing sample mislabeled", err)
	}
	s.Runtime = strings.Repeat("2", 32)
	if _, err := inputAfterLookup([]memory.Record{encode(s)}, e, 0, 1501); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) {
		t.Fatal("target change mislabeled", err)
	}
	s.Runtime = e.Runtime
	s.InputBlocked = nil
	if _, err := inputAfterLookup([]memory.Record{encode(s)}, e, 0, 1501); errors.Is(err, ErrInputObservationStale) {
		t.Fatal("invalid sample mislabeled")
	}
}

type expiredThenReadyPeer struct {
	*stopReconcilePeer
	observations  int
	changeRuntime bool
	discoveryAt   int
}

func (p *expiredThenReadyPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	p.observations++
	if p.observations == 1 {
		return InputObservation{}, ErrInputObservationStale
	}
	if p.observations == 2 && p.changeRuntime {
		p.current.Runtime = strings.Repeat("2", 32)
		p.current.NextSlot = 1
		p.current.Owner = ""
	}
	return p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
}
func (p *expiredThenReadyPeer) RuntimeCandidate(ctx context.Context, i Identity) (*Identity, error) {
	if p.discoveryAt == 0 {
		p.discoveryAt = p.observations
	}
	return p.stopReconcilePeer.RuntimeCandidate(ctx, i)
}

func TestExpiredLookupRetriesInputWithoutEagerDiscovery(t *testing.T) {
	d, base := stopReconcileFixture(t)
	p := &expiredThenReadyPeer{stopReconcilePeer: base}
	d.Backend = p
	d.State.Bound = false
	if err := stopReconcileContinueFor(d, 4*time.Second); err != nil {
		t.Fatal(err)
	}
	if !d.State.Bound || p.observations != 2 || p.discoveries != 0 || len(p.inputs) != 1 {
		t.Fatalf("bound=%v observations=%d discoveries=%d inputs=%d", d.State.Bound, p.observations, p.discoveries, len(p.inputs))
	}
}

func TestExpiredLookupClosingAlsoRefreshesInput(t *testing.T) {
	d, base := stopReconcileFixture(t)
	p := &expiredThenReadyPeer{stopReconcilePeer: base}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := stopReconcileContinueFor(d, 4*time.Second); err != nil {
		t.Fatal(err)
	}
	if !d.State.Closed || p.observations != 2 || p.discoveries != 0 || len(p.inputs) != 1 {
		t.Fatalf("closed=%v observations=%d discoveries=%d inputs=%d", d.State.Closed, p.observations, p.discoveries, len(p.inputs))
	}
}

func TestExpiredThenMissingLookupStillRecoversRuntime(t *testing.T) {
	for _, reload := range []bool{false, true} {
		name := "first_bind"
		if reload {
			name = "reload"
		}
		t.Run(name, func(t *testing.T) {
			d, base := stopReconcileFixture(t)
			p := &expiredThenReadyPeer{stopReconcilePeer: base, changeRuntime: true}
			d.Backend = p
			if reload {
				if err := d.RequestReload(context.Background(), "recover"); err != nil {
					t.Fatal(err)
				}
			} else {
				d.State.Bound = false
			}
			if err := stopReconcileContinueFor(d, 4*time.Second); err != nil {
				t.Fatal(err)
			}
			if !d.State.Bound || d.State.Identity.Runtime != p.current.Runtime || p.discoveryAt != 2 {
				t.Fatalf("runtime recovery lost: %+v discoveries=%d at=%d", d.State.Identity, p.discoveries, p.discoveryAt)
			}
			if reload && d.State.Reload.Phase != "complete" {
				t.Fatal("reload incomplete")
			}
			for _, input := range p.inputs {
				if input.Envelope.Runtime != p.current.Runtime || input.Kind == "reload" {
					t.Fatal("sent stale-runtime input")
				}
			}
		})
	}
}
