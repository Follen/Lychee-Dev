//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/desktop"
)

// Exercise the actual asynchronous reader and cache-off Source traversal. If
// evidence samples its clock outside the tracker lock, the reader can accept a
// newer legal frame before evidence checks the earlier clock against that frame.
func TestInputSignalEvidenceClockCannotPrecedeConcurrentAcceptedFrame(t *testing.T) {
	signal, frames := edgeSignalFixture(t)
	edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
	edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", true, 0)})
	native, source, envelope, write := cadenceFixture(t)
	native.Hints = nil
	write(512<<10, 2050, nil)
	var uptime atomic.Int64
	uptime.Store(2100)
	var clockCalls, lockedClockCalls int
	clock := func() (int64, error) {
		clockCalls++
		now, err := desktop.CaptureSystemTicks()
		if err != nil {
			return now, err
		}
		if signal.mu.TryLock() {
			signal.mu.Unlock()
			// The old order exposes this interval to the actual reader. The
			// frame is stamped after now and fully accepted before returning.
			edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
		} else {
			lockedClockCalls++
		}
		return now, nil
	}
	var gates nativeInputOpticalCollector
	signalChecks, lookups := 0, 0
	observation, err := observeHybridInputWithObserver(func() (InputSignalEvidence, error) {
		signalChecks++
		if signalChecks == 1 {
			return signal.evidence()
		}
		return signal.evidenceWithClock(clock)
	}, func() (InputObservation, error) {
		lookups++
		return native.observeInputFrom(context.Background(), source, envelope, 0, uptime.Load, time.Now)
	}, gates.Observe)
	if err != nil || observation.Optical == nil || observation.SampleMillis != 2050 {
		t.Fatalf("a concurrent legal frame was rejected after fresh memory: error=%v optical=%v", err, observation.Optical != nil)
	}
	if clockCalls != 1 || lockedClockCalls != 1 || signalChecks != 2 || lookups != 1 || source.regions.Load() != 1 || native.Hints != nil {
		t.Fatalf("clock/observation ordering changed: clock=%d locked=%d signal=%d lookups=%d regions=%d", clockCalls, lockedClockCalls, signalChecks, lookups, source.regions.Load())
	}
	stats := gates.Snapshot()
	if stats.Pre.Checks != 1 || stats.Pre.Accepted != 1 || stats.Post.Checks != 1 || stats.Post.Accepted != 1 {
		t.Fatalf("observer received extra calls or rejected the frame: %+v", stats)
	}
}

func TestInputSignalLockedClockPreservesFailureGates(t *testing.T) {
	for _, kind := range []string{"clock_error", "future", "replay", "expired", "after_input"} {
		t.Run(kind, func(t *testing.T) {
			signal, frames := edgeSignalFixture(t)
			edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, 0)})
			last := edgeSignalFrame(t, "ready", true, 0)
			edgeFeed(t, frames, edgeFrameArrival{frame: last})
			wantReason := "input_signal_unavailable"
			clockFailure := errors.New("fixture.clock_failed")
			switch kind {
			case "future":
				edgeFeed(t, frames, edgeFrameArrival{frame: edgeSignalFrame(t, "ready", false, time.Second)})
			case "replay":
				edgeFeed(t, frames, edgeFrameArrival{frame: last})
			case "after_input":
				signal.afterInput()
				wantReason = "input_signal_waiting"
			}
			calls, locked := 0, 0
			_, err := signal.evidenceWithClock(func() (int64, error) {
				calls++
				if signal.mu.TryLock() {
					signal.mu.Unlock()
				} else {
					locked++
				}
				if kind == "clock_error" {
					return 0, clockFailure
				}
				if kind == "expired" {
					return last.SystemTicks + 501*signalTicksPerMillisecond, nil
				}
				return desktop.CaptureSystemTicks()
			})
			if calls != 1 || locked != 1 || !errors.Is(err, ErrPending) {
				t.Fatalf("clock count, locking or pending fact changed: calls=%d locked=%d error=%v", calls, locked, err)
			}
			if kind == "clock_error" {
				if !errors.Is(err, clockFailure) {
					t.Fatal("clock failure was lost", err)
				}
				return
			}
			var pending *inputSignalPending
			if !errors.As(err, &pending) || pending.reason != wantReason {
				t.Fatalf("failure gate changed: error=%v expected=%s", err, wantReason)
			}
		})
	}
}
