package channel

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type signalWaitPeer struct {
	*stopReconcilePeer
	observations  int
	persistent    bool
	reasons       []string
	persistReason string
	started       time.Time
	discoveryAt   time.Duration
}

func (p *signalWaitPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	p.observations++
	if p.observations <= len(p.reasons) {
		return InputObservation{}, &inputSignalPending{p.reasons[p.observations-1]}
	}
	if p.observations == 1 || p.persistent {
		if p.persistReason != "" {
			return InputObservation{}, &inputSignalPending{p.persistReason}
		}
		return InputObservation{}, &inputSignalPending{"input_signal_waiting"}
	}
	s, err := p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
	s.SampleMillis = 1000
	s.Optical = &InputSignalEvidence{State: "ready", FrameTicks: signalTicks(1000), EdgeTicks: signalTicks(950)}
	return s, err
}

func (p *signalWaitPeer) RuntimeCandidate(ctx context.Context, identity Identity) (*Identity, error) {
	p.discoveryAt = time.Since(p.started)
	p.discoveries++
	// An expensive discovery owns the rest of this invocation. Cancel rather
	// than sleep so the test detects loss of the next input turn immediately.
	p.cancel()
	return nil, ctx.Err()
}

func TestSignalWaitingRetriesBoundInputBeforeRuntimeDiscovery(t *testing.T) {
	d, base := stopReconcileFixture(t)
	d.State.Identity.InputState = bridge.InputSignalCapability
	p := &signalWaitPeer{stopReconcilePeer: base}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	p.cancel, p.started = cancel, time.Now()
	err := d.Continue(ctx)
	if err != nil || !d.State.Closed || p.observations != 2 || p.discoveries != 0 || len(p.inputs) != 1 {
		t.Fatalf("optical retry lost to discovery: err=%v closed=%v observations=%d discoveries=%d inputs=%d", err, d.State.Closed, p.observations, p.discoveries, len(p.inputs))
	}
	if d.State.Input == nil || d.State.Input.Outcome == nil || d.State.Input.Outcome.Disposition != "submitted" || d.State.Input.Observation.Optical == nil {
		t.Fatal("input bypassed the journal or optical observation")
	}
}

func TestSignalUnavailableThenWaitingRetriesBoundInputBeforeRuntimeDiscovery(t *testing.T) {
	d, base := stopReconcileFixture(t)
	d.State.Identity.InputState = bridge.InputSignalCapability
	p := &signalWaitPeer{stopReconcilePeer: base, reasons: []string{"input_signal_unavailable", "input_signal_waiting"}}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	p.cancel, p.started = cancel, time.Now()
	err := d.Continue(ctx)
	if err != nil || !d.State.Closed || p.observations != 3 || p.discoveries != 0 || len(p.inputs) != 1 {
		t.Fatalf("transient unavailable lost the next optical turns to discovery: err=%v closed=%v observations=%d discoveries=%d inputs=%d", err, d.State.Closed, p.observations, p.discoveries, len(p.inputs))
	}
	if d.State.Input == nil || d.State.Input.Outcome == nil || d.State.Input.Outcome.Disposition != "submitted" || d.State.Input.Observation.Optical == nil {
		t.Fatal("input bypassed the journal or optical observation")
	}
}

func TestSignalUnavailablePersistentBoundInputStillDiscovers(t *testing.T) {
	d, base := stopReconcileFixture(t)
	d.State.Identity.InputState = bridge.InputSignalCapability
	p := &signalWaitPeer{stopReconcilePeer: base, persistent: true, persistReason: "input_signal_unavailable", reasons: []string{"input_signal_unavailable", "input_signal_waiting", "input_signal_unavailable", "input_signal_waiting"}}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	p.cancel, p.started = cancel, time.Now()
	err := d.Continue(ctx)
	if !errors.Is(err, ErrPending) || p.discoveries != 1 || len(p.inputs) != 0 || d.State.Closed {
		t.Fatalf("unavailable suppressed fallback or sent input: err=%v discoveries=%d inputs=%d closed=%v", err, p.discoveries, len(p.inputs), d.State.Closed)
	}
	if p.discoveryAt < 2*time.Second || p.observations < 2 {
		t.Fatalf("fixed grace renewed or missing: elapsed=%s observations=%d", p.discoveryAt, p.observations)
	}
}

func TestSignalWaitingPersistentBoundInputStillDiscovers(t *testing.T) {
	d, base := stopReconcileFixture(t)
	d.State.Identity.InputState = bridge.InputSignalCapability
	p := &signalWaitPeer{stopReconcilePeer: base, persistent: true}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	p.cancel, p.started = cancel, time.Now()
	err := d.Continue(ctx)
	if !errors.Is(err, ErrPending) || p.discoveries != 1 || len(p.inputs) != 0 || d.State.Closed {
		t.Fatalf("optical wait suppressed fallback or sent input: err=%v discoveries=%d inputs=%d closed=%v", err, p.discoveries, len(p.inputs), d.State.Closed)
	}
	if p.discoveryAt < 2*time.Second || p.observations < 2 {
		t.Fatalf("fixed grace renewed or missing: elapsed=%s observations=%d", p.discoveryAt, p.observations)
	}
}

func signalGraceDriver() *Driver {
	d := &Driver{}
	d.State.Bound = true
	d.State.Identity.Runtime = "runtime"
	d.State.Identity.InputState = bridge.InputSignalCapability
	d.State.Transaction = &Transaction{Envelope: bridge.SlotEnvelope{Runtime: "runtime", Nonce: "exchange"}, Phase: "published"}
	d.Waiting = "input_signal_waiting"
	return d
}

func TestSignalWaitingGraceDoesNotRenew(t *testing.T) {
	d := signalGraceDriver()
	start := time.Unix(1000, 0)
	var grace inputSignalWaitGrace
	if !grace.deferDiscovery(d, start) {
		t.Fatal("eligible exchange did not get observation window")
	}
	deadline := grace.until
	for _, elapsed := range []time.Duration{250 * time.Millisecond, time.Second, 1999 * time.Millisecond, 2 * time.Second, 3 * time.Second} {
		if elapsed == time.Second || elapsed == 2*time.Second {
			d.Waiting = "input_signal_unavailable"
		} else {
			d.Waiting = "input_signal_waiting"
		}
		if got := grace.deferDiscovery(d, start.Add(elapsed)); got != (elapsed < 2*time.Second) || grace.until != deadline {
			t.Fatalf("repeated waiting renewed deadline: elapsed=%s defer=%v deadline=%v", elapsed, got, grace.until)
		}
	}
	// A different pending condition cannot renew this exchange's expired grace.
	d.Waiting = "input_signal_unavailable"
	if grace.deferDiscovery(d, start.Add(4*time.Second)) {
		t.Fatal("unavailable restarted expired exchange grace")
	}
	d.Waiting = "input_signal_waiting"
	if grace.deferDiscovery(d, start.Add(5*time.Second)) || grace.until != deadline {
		t.Fatal("interleaved blocker restarted expired grace")
	}
	d.State.Transaction.Envelope.Nonce = "next-exchange"
	if !grace.deferDiscovery(d, start.Add(6*time.Second)) || grace.until != start.Add(8*time.Second) {
		t.Fatal("new exchange did not get its own fixed window")
	}
}

func TestSignalWaitingGraceOnlyNormalBoundPublishedExchange(t *testing.T) {
	for name, change := range map[string]func(*Driver){
		"unbound":     func(d *Driver) { d.State.Bound = false },
		"unknown":     func(d *Driver) { d.Waiting = "input_signal_unknown" },
		"memory":      func(d *Driver) { d.State.Identity.InputState = "lycheedev.input.v1" },
		"no_exchange": func(d *Driver) { d.State.Transaction = nil },
		"uncertain":   func(d *Driver) { d.State.Transaction.Phase = "input_attempted" },
		"old_runtime": func(d *Driver) { d.State.Transaction.Envelope.Runtime = "old" },
		"reload":      func(d *Driver) { d.State.Reload = &ReloadAttempt{Phase: "intent"} },
		"recovery":    func(d *Driver) { d.State.Recovery = &RuntimeRecovery{Phase: "intent"} },
	} {
		for _, reason := range []string{"input_signal_waiting", "input_signal_unavailable"} {
			t.Run(name+"/"+reason, func(t *testing.T) {
				d := signalGraceDriver()
				d.Waiting = reason
				change(d)
				var grace inputSignalWaitGrace
				if grace.deferDiscovery(d, time.Unix(1000, 0)) || !grace.until.IsZero() {
					t.Fatal("non-optical/normal exchange acquired discovery grace")
				}
			})
		}
	}
}
