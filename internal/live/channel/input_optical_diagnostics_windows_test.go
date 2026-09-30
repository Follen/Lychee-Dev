//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/memory"
)

type opticalScanGateSource struct {
	memory.Source
	once    sync.Once
	started chan struct{}
	release chan struct{}
}

func (s *opticalScanGateSource) Read(ctx context.Context, address uint64, out []byte) (int, error) {
	s.once.Do(func() {
		close(s.started)
		select {
		case <-s.release:
		case <-ctx.Done():
		}
	})
	return s.Source.Read(ctx, address, out)
}

func TestHybridOpticalDiagnosticsSeparatePostScanFailures(t *testing.T) {
	for _, outcome := range []string{"waiting", "unavailable", "changed", "accepted", "frameExpired"} {
		t.Run(outcome, func(t *testing.T) {
			signal, frames := edgeSignalFixture(t)
			edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
			edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
			native, source, envelope, write := cadenceFixture(t)
			native.Hints = nil
			write(512<<10, 2050, nil)
			var uptime atomic.Int64
			uptime.Store(2100)
			gated := &opticalScanGateSource{Source: source, started: make(chan struct{}), release: make(chan struct{})}
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			var diagnostics nativeInputOpticalCollector
			type result struct {
				observation InputObservation
				err         error
				memoryFresh bool
			}
			completed := make(chan result, 1)
			go func() {
				fresh := false
				observation, err := observeHybridInputWithObserver(signal.evidence, func() (InputObservation, error) {
					s, err := native.observeInputFrom(ctx, gated, envelope, 0, uptime.Load, time.Now)
					fresh = err == nil
					return s, err
				}, diagnostics.Observe)
				completed <- result{observation, err, fresh}
			}()
			select {
			case <-gated.started:
			case <-ctx.Done():
				t.Fatal("real cache-off memory traversal did not reach source barrier")
			}
			// Optical capture changes while the actual memory traversal is held.
			switch outcome {
			case "unavailable":
				edgeFeed(t, frames, edgeFrameArrival{})
			case "waiting":
				edgeFeed(t, frames, edgeFrameArrival{})
				edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
			case "changed":
				edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "focus", false, 0)})
			case "frameExpired":
				// Keep the actual capture quiet past its existing 500ms gate,
				// while memory publishes a new matching sample before reading.
				<-time.After(510 * time.Millisecond)
				uptime.Store(2650)
				write(512<<10, 2600, nil)
			}
			close(gated.release)
			var got result
			select {
			case got = <-completed:
			case <-ctx.Done():
				t.Fatal("hybrid source/capture fixture did not return")
			}
			if !got.memoryFresh || source.regions.Load() == 0 || native.Hints != nil {
				t.Fatal("fixture skipped actual full lookup or failed memory freshness", got)
			}
			stats := diagnostics.Snapshot()
			if stats.Pre.Checks != 1 || stats.Pre.Accepted != 1 || stats.Post.Checks != 1 {
				t.Fatalf("hybrid gate diagnostics missing: %+v", stats)
			}
			if outcome == "accepted" {
				if got.err != nil || got.observation.Optical == nil || stats.Post.Accepted != 1 {
					t.Fatal("healthy gate rejected or not counted", got, stats)
				}
				return
			}
			var pending *inputSignalPending
			category := outcome
			if outcome == "frameExpired" {
				category = "unavailable"
			}
			if !errors.As(got.err, &pending) || pending.reason != "input_signal_"+category || got.observation.Optical != nil || stats.Post.Accepted != 0 {
				t.Fatal("post gate failure changed input authority", got, stats)
			}
			if category == "waiting" && stats.Post.Waiting != 1 || category == "unavailable" && stats.Post.Unavailable != 1 || category == "changed" && stats.Post.Changed != 1 {
				t.Fatal("post lookup causes were conflated", outcome, stats)
			}
		})
	}
}

func TestInputOpticalCollectorFixedOutcomes(t *testing.T) {
	var collector nativeInputOpticalCollector
	outcomes := []error{nil, &inputSignalPending{"input_signal_unavailable"}, &inputSignalPending{"input_signal_waiting"}, &inputSignalPending{"input_combat_lockdown"}, &inputSignalPending{"input_signal_unknown"}, &inputSignalPending{"input_signal_changed"}, errors.New("fixture.unclassified")}
	for _, post := range []bool{false, true} {
		for _, err := range outcomes {
			collector.Observe(post, errors.Join(err))
		}
	}
	snapshot := collector.Snapshot()
	for _, gate := range []InputOpticalGateDiagnostics{snapshot.Pre, snapshot.Post} {
		if gate.Checks != 7 || gate.Accepted != 1 || gate.Unavailable != 1 || gate.Waiting != 1 || gate.Blocked != 1 || gate.Unknown != 1 || gate.Changed != 1 || gate.Other != 1 {
			t.Fatal("outcome was dropped or counted twice", gate)
		}
	}
	snapshot.Pre.Checks = 0
	if collector.Snapshot().Pre.Checks != 7 {
		t.Fatal("snapshot exposed mutable collector state")
	}
}

func TestHybridOpticalObserverDoesNotChangeEarlyFailures(t *testing.T) {
	for _, stage := range []string{"pre", "memory", "post"} {
		t.Run(stage, func(t *testing.T) {
			failure := errors.New("fixture.exact_failure")
			var collector nativeInputOpticalCollector
			calls, lookups := 0, 0
			_, err := observeHybridInputWithObserver(func() (InputSignalEvidence, error) {
				calls++
				if stage == "pre" || stage == "post" && calls == 2 {
					return InputSignalEvidence{}, failure
				}
				return InputSignalEvidence{State: "ready"}, nil
			}, func() (InputObservation, error) {
				lookups++
				if stage == "memory" {
					return InputObservation{}, failure
				}
				blocked := false
				return InputObservation{InputBlocked: &blocked}, nil
			}, collector.Observe)
			if err != failure {
				t.Fatal("observer changed original error identity", err)
			}
			stats := collector.Snapshot()
			if stats.Pre.Checks != 1 || stage == "pre" && (lookups != 0 || calls != 1 || stats.Pre.Other != 1 || stats.Post.Checks != 0) || stage == "memory" && (lookups != 1 || calls != 1 || stats.Pre.Accepted != 1 || stats.Post.Checks != 0) || stage == "post" && (lookups != 1 || calls != 2 || stats.Pre.Accepted != 1 || stats.Post.Other != 1) {
				t.Fatal("observer changed gate order or added a check", stage, calls, lookups, stats)
			}
		})
	}
}

func TestInputOpticalCollectorConcurrentSnapshots(t *testing.T) {
	var collector nativeInputOpticalCollector
	var workers sync.WaitGroup
	for worker := 0; worker < 4; worker++ {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for i := 0; i < 100; i++ {
				collector.Observe(false, nil)
				collector.Observe(true, &inputSignalPending{"input_signal_waiting"})
				_ = collector.Snapshot()
			}
		}()
	}
	workers.Wait()
	stats := collector.Snapshot()
	if stats.Pre.Checks != 400 || stats.Pre.Accepted != 400 || stats.Post.Checks != 400 || stats.Post.Waiting != 400 {
		t.Fatal("concurrent aggregates lost measurements", stats)
	}
}
