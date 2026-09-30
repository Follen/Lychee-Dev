package channel

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type runtimeCooldownPeer struct {
	*stopReconcilePeer
	observations  int
	persistent    bool
	cancelNext    bool
	firstFinished time.Time
	firstElapsed  time.Duration
	secondStarted time.Time
}

func (p *runtimeCooldownPeer) ObserveInput(ctx context.Context, e bridge.SlotEnvelope, after int64, capability string) (InputObservation, error) {
	p.observations++
	if p.observations == 2 && p.cancelNext {
		p.cancel()
		return InputObservation{}, ctx.Err()
	}
	if p.observations == 1 || p.persistent {
		return InputObservation{}, ErrPending
	}
	if p.observations < 4 {
		timer := time.NewTimer(700 * time.Millisecond)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return InputObservation{}, ctx.Err()
		case <-timer.C:
			return InputObservation{}, ErrPending
		}
	}
	return p.stopReconcilePeer.ObserveInput(ctx, e, after, capability)
}

func (p *runtimeCooldownPeer) RuntimeCandidate(ctx context.Context, _ Identity) (*Identity, error) {
	p.discoveries++
	if p.discoveries > 1 {
		p.secondStarted = time.Now()
		if p.persistent {
			p.cancel()
		}
		// A second cold discovery owns the remaining caller budget. It cannot
		// promote the absent evidence or grant input to make this test pass.
		<-ctx.Done()
		return nil, ctx.Err()
	}
	started := time.Now()
	timer := time.NewTimer(1050 * time.Millisecond)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-timer.C:
		p.firstFinished = time.Now()
		p.firstElapsed = time.Since(started)
		return nil, nil
	}
}

func runtimeCooldownFixture(t *testing.T) (*Driver, *runtimeCooldownPeer) {
	t.Helper()
	d, base := stopReconcileFixture(t)
	d.State.Identity.InputState = "lycheedev.input.v1"
	p := &runtimeCooldownPeer{stopReconcilePeer: base}
	d.Backend = p
	if err := d.RequestClose(context.Background()); err != nil {
		t.Fatal(err)
	}
	return d, p
}

func TestRuntimeDiscoveryCooldownAllowsFiniteProtocolProgress(t *testing.T) {
	d, p := runtimeCooldownFixture(t)
	deadline := d.State.CloseBudget.DeadlineMS
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	p.cancel = cancel
	err := d.Continue(ctx)
	if err != nil || !d.State.Closed || p.discoveries != 1 || p.observations != 4 || len(p.inputs) != 1 {
		t.Fatalf("cold rediscovery consumed the protocol progress window: err=%v closed=%v discoveries=%d observations=%d inputs=%d", err, d.State.Closed, p.discoveries, p.observations, len(p.inputs))
	}
	if d.State.CloseBudget.DeadlineMS != deadline || d.State.Input == nil || d.State.Input.Outcome == nil || d.State.Input.Outcome.Disposition != "submitted" {
		t.Fatal("progress renewed the goal deadline or bypassed the input journal")
	}
}

func TestRuntimeDiscoveryCooldownPersistentMissingStillDiscovers(t *testing.T) {
	d, p := runtimeCooldownFixture(t)
	p.persistent = true
	deadline := d.State.CloseBudget.DeadlineMS
	ctx, cancel := context.WithTimeout(context.Background(), 7*time.Second)
	defer cancel()
	p.cancel = cancel
	err := d.Continue(ctx)
	if !errors.Is(err, ErrPending) || p.discoveries != 2 || len(p.inputs) != 0 || d.State.Closed || d.State.CloseBudget.DeadlineMS != deadline {
		t.Fatal("missing evidence lost bounded discovery or authorized input", err, p.discoveries, p.inputs)
	}
	if p.secondStarted.Sub(p.firstFinished) < 3*p.firstElapsed || p.observations < 2 {
		t.Fatal("discovery cooldown started before completion or did not leave protocol turns", p.secondStarted.Sub(p.firstFinished), p.firstElapsed)
	}
}

func TestRuntimeDiscoveryCooldownCancellationIsImmediate(t *testing.T) {
	d, p := runtimeCooldownFixture(t)
	p.cancelNext = true
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	p.cancel = cancel
	err := d.Continue(ctx)
	if !errors.Is(err, context.Canceled) || p.discoveries != 1 || len(p.inputs) != 0 || d.State.Closed {
		t.Fatal("cooldown delayed cancellation or replayed input", err, p.discoveries, p.inputs)
	}
	if time.Since(p.firstFinished) >= time.Second {
		t.Fatal("cancellation waited for the remaining discovery cooldown")
	}
}

func TestRuntimeDiscoveryCooldownBoundsLeaveBindingRecoveryUnchanged(t *testing.T) {
	for _, tc := range []struct{ elapsed, want time.Duration }{
		{0, time.Second}, {25 * time.Millisecond, time.Second}, {999 * time.Millisecond, time.Second},
		{time.Second, 3 * time.Second}, {7 * time.Second, 21 * time.Second}, {9 * time.Second, 27 * time.Second},
		{10 * time.Second, 30 * time.Second}, {time.Minute, 30 * time.Second}, {time.Duration(1<<63 - 1), 30 * time.Second},
	} {
		if got := runtimeDiscoveryCooldown(tc.elapsed); got != tc.want {
			t.Fatalf("elapsed=%s cooldown=%s want=%s", tc.elapsed, got, tc.want)
		}
	}
	if discoveryCooldown(7*time.Second) != 7*time.Second || discoveryCooldown(25*time.Millisecond) != time.Second {
		t.Fatal("successful RecoverBinding inherited the runtime-search multiplier")
	}
}

type runtimeCooldownClosingPeer struct {
	*runtimeCooldownPeer
	preflights int
}

func (p *runtimeCooldownClosingPeer) closeReloadRuntime(ctx context.Context, d *Driver) (bool, error) {
	p.preflights++
	p.cancel()
	return false, ctx.Err()
}

func TestRuntimeDiscoveryCooldownClosingExplicitReloadPreflightIsImmediate(t *testing.T) {
	d, base := runtimeCooldownFixture(t)
	p := &runtimeCooldownClosingPeer{runtimeCooldownPeer: base}
	d.Backend = p
	if err := d.RequestReload(context.Background(), "cooldown-close"); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	p.cancel = cancel
	started := time.Now()
	err := d.Continue(ctx)
	if !errors.Is(err, context.Canceled) || p.preflights != 1 || p.discoveries != 0 || len(p.inputs) != 0 || time.Since(started) >= time.Second {
		t.Fatal("closing explicit reload preflight entered the ordinary discovery cooldown", err, p.preflights, p.discoveries, p.inputs)
	}
}

type bindingPrefixCooldownPeer struct {
	pendingBackend
	bootstrapObservations, discoveries int
	firstFinished, secondStarted       time.Time
}

func (p *bindingPrefixCooldownPeer) Observe(ctx context.Context, q ObservationQuery) (Observation, error) {
	if q.Kind == "bootstrap_changed" {
		p.bootstrapObservations++
		if p.bootstrapObservations == 1 {
			timer := time.NewTimer(1050 * time.Millisecond)
			defer timer.Stop()
			select {
			case <-ctx.Done():
				return Observation{}, ctx.Err()
			case <-timer.C:
			}
		}
	}
	return Observation{}, ErrPending
}

func (p *bindingPrefixCooldownPeer) RuntimeCandidate(ctx context.Context, _ Identity) (*Identity, error) {
	p.discoveries++
	if p.discoveries == 2 {
		p.secondStarted = time.Now()
		p.cancel()
		return nil, ctx.Err()
	}
	timer := time.NewTimer(25 * time.Millisecond)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-timer.C:
	}
	p.firstFinished = time.Now()
	return nil, nil
}

func TestRuntimeDiscoveryCooldownExcludesFailedBindingRecoveryCost(t *testing.T) {
	d, _ := stopReconcileFixture(t)
	d.State.Bound = false
	p := &bindingPrefixCooldownPeer{}
	d.Backend = p
	ctx, cancel := context.WithTimeout(context.Background(), 7*time.Second)
	defer cancel()
	p.cancel = cancel
	err := d.Continue(ctx)
	interval := p.secondStarted.Sub(p.firstFinished)
	if !errors.Is(err, ErrPending) || p.discoveries != 2 || p.bootstrapObservations != 2 || p.sends != 1 || d.State.Bound || interval < time.Second || interval >= 2500*time.Millisecond {
		t.Fatal("failed binding recovery inflated a cheap candidate's cooldown or replayed bind", err, interval, p.discoveries, p.bootstrapObservations, p.sends)
	}
}
