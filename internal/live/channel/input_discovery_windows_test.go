//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestCacheOffStalePrefixDoesNotHideFreshSample(t *testing.T) {
	n, source, envelope, write := cadenceFixture(t)
	n.Hints = nil
	// Every call has a new retained stale prefix before a genuinely fresh
	// publication. A progress cursor cannot fix this continuously moving prefix.
	for round := int64(0); round < 3; round++ {
		now := int64(2100) + round*1000
		write(512<<10, now-1100, nil)
		write((512<<10)+4096, now-100, nil)
		sample, err := n.observeInputFrom(context.Background(), source, envelope, 0,
			func() int64 { return now }, time.Now)
		if err != nil || sample.SampleMillis != now-100 || sample.Address != (512<<10)+4096 {
			t.Fatalf("round=%d cache-off starved fresh publication: sample=%+v err=%v regionEnumerations=%d", round, sample, err, source.regions.Load())
		}
		if n.Hints != nil || source.regions.Load() != int32(round+1) {
			t.Fatal("cache-off reused a hidden address cache or repeated full discovery")
		}
	}
}

func TestCacheOffSampleExpiryDuringScannerDrainCannotAuthorizeInput(t *testing.T) {
	n, source, envelope, write := cadenceFixture(t)
	n.Hints = nil
	write(512<<10, 2050, nil)
	var calls atomic.Int32
	clock := func() int64 {
		if calls.Add(1) == 1 {
			return 2100
		}
		return 2601
	}
	sample, err := n.observeInputFrom(context.Background(), source, envelope, 1950, clock, time.Now)
	if !errors.Is(err, ErrInputObservationStale) || sample.Address != 0 || n.Hints != nil || source.regions.Load() != 1 {
		t.Fatalf("expired candidate authorized input: sample=%+v err=%v", sample, err)
	}
}

func TestCacheOffStaleMetadataRechecksClockAfterDiscovery(t *testing.T) {
	n, source, envelope, _ := cadenceFixture(t)
	n.Hints = nil
	var calls atomic.Int32
	clock := func() int64 {
		if calls.Add(1) == 1 {
			return 1600
		}
		return 500
	}
	sample, err := n.observeInputFrom(context.Background(), source, envelope, 0, clock, time.Now)
	if !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) || sample.Address != 0 || n.inputWait.key != "" || n.inputHintMillis != 0 {
		t.Fatalf("clock rollback promoted stale metadata: sample=%+v err=%v", sample, err)
	}
}

func TestCacheOffDeferredScanStillChecksSharedBudget(t *testing.T) {
	n, source, envelope, _ := cadenceFixture(t)
	n.Hints = nil
	now := time.Unix(100, 0)
	observe := func() (InputObservation, error) {
		return n.observeInputFrom(context.Background(), source, envelope, 0, func() int64 { return 1600 }, func() time.Time { return now })
	}
	if _, err := observe(); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	n.observation.session = memory.NewSession(memory.Budget{MaxReadCalls: 1})
	measured := n.memorySource(source)
	_, _ = measured.Read(context.Background(), 0, make([]byte, 1))
	_, _ = measured.Read(context.Background(), 0, make([]byte, 1))
	if _, err := observe(); !errors.Is(err, memory.ErrBudget) || source.regions.Load() != 1 {
		t.Fatalf("deferred scan hid shared budget: %v", err)
	}
}

func TestCacheOffStaleOnlyDefersDiscoveryWithoutRetainingAddress(t *testing.T) {
	n, source, envelope, write := cadenceFixture(t)
	n.Hints = nil
	now := time.Unix(100, 0)
	tick := int64(1600)
	observe := func(ctx context.Context) (InputObservation, error) {
		return n.observeInputFrom(ctx, source, envelope, 0, func() int64 { return tick }, func() time.Time { return now })
	}
	sample, err := observe(context.Background())
	if !errors.Is(err, ErrInputObservationStale) || sample.Address != 0 || source.regions.Load() != 1 {
		t.Fatalf("stale-only initial sample=%+v err=%v regions=%d", sample, err, source.regions.Load())
	}
	deadline := n.inputWait.deadline
	for i := 0; i < 6; i++ {
		now = now.Add(250 * time.Millisecond)
		tick += 250
		write(512<<10, tick-600, nil)
		sample, err = observe(context.Background())
		if !errors.Is(err, ErrInputObservationStale) || sample.Address != 0 || source.regions.Load() != 1 || n.Hints != nil || n.inputWait.deadline != deadline {
			t.Fatalf("repeated stale discovery or renewed grace: sample=%+v err=%v regions=%d", sample, err, source.regions.Load())
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err = observe(ctx); !errors.Is(err, context.Canceled) || source.regions.Load() != 1 {
		t.Fatalf("wait masked cancellation: %v", err)
	}
	now = deadline
	tick = 3600
	write(512<<10, 3000, nil)
	if _, err = observe(context.Background()); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) || source.regions.Load() != 2 || n.inputWait.deadline != deadline {
		t.Fatalf("new expired record renewed grace: %v regions=%d", err, source.regions.Load())
	}
	if len(n.Lookups) != 2 || !n.Lookups[1].Coverage.Complete {
		t.Fatal("stale-only full fallback lost its coverage")
	}
	write((512<<10)+4096, 3500, nil)
	sample, err = observe(context.Background())
	if err != nil || sample.SampleMillis != 3500 || source.regions.Load() != 3 || n.Hints != nil {
		t.Fatalf("fresh recovery did not use new full discovery: sample=%+v err=%v regions=%d", sample, err, source.regions.Load())
	}
}
