//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"
)

// The real reader consumes frames independently while actual cache-off Source
// traversal is blocked. Expired optical evidence must still deny fresh memory,
// but must not erase the heartbeat comparison needed by the next fresh edge.
func TestInputSignalExpiryPreservesNextFreshEdgeDuringMemoryScan(t *testing.T) {
	signal, frames := edgeSignalFixture(t)
	edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	before, err := signal.evidence()
	if err != nil {
		t.Fatal(err)
	}
	native, source, envelope, write := cadenceFixture(t)
	native.Hints = nil
	write(512<<10, 2050, nil)
	var uptime atomic.Int64
	uptime.Store(2100)
	gated := &opticalScanGateSource{Source: source, started: make(chan struct{}), release: make(chan struct{})}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	var diagnostics nativeInputOpticalCollector
	type result struct {
		err         error
		memoryFresh bool
	}
	completed := make(chan result, 1)
	go func() {
		fresh := false
		_, err := observeHybridInputWithObserver(signal.evidence, func() (InputObservation, error) {
			s, err := native.observeInputFrom(ctx, gated, envelope, 0, uptime.Load, time.Now)
			fresh = err == nil
			return s, err
		}, diagnostics.Observe)
		completed <- result{err, fresh}
	}()
	select {
	case <-gated.started:
	case <-ctx.Done():
		t.Fatal("actual Source scan did not start")
	}
	<-time.After(510 * time.Millisecond)
	uptime.Store(2650)
	write(512<<10, 2600, nil)
	close(gated.release)
	var rejected result
	select {
	case rejected = <-completed:
	case <-ctx.Done():
		t.Fatal("memory traversal did not finish")
	}
	var pending *inputSignalPending
	if !rejected.memoryFresh || !errors.As(rejected.err, &pending) || pending.reason != "input_signal_unavailable" || diagnostics.Snapshot().Post.Unavailable != 1 {
		t.Fatal("expired optical frame failed to reject fresh memory", rejected)
	}
	edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	after, err := signal.evidence()
	if err != nil || after.EdgeTicks <= before.EdgeTicks {
		t.Fatalf("fresh toggled frame lost comparison history after expiry: evidence=%+v error=%v oldEdge=%d", after, err, before.EdgeTicks)
	}
	observation, err := observeHybridInput(signal.evidence, func() (InputObservation, error) {
		return native.observeInputFrom(ctx, source, envelope, 0, uptime.Load, time.Now)
	})
	if err != nil || observation.Optical == nil || observation.Optical.EdgeTicks <= before.EdgeTicks || observation.SampleMillis != 2600 || native.Hints != nil {
		t.Fatal("new optical edge did not permit independently fresh memory", observation, err)
	}
}
