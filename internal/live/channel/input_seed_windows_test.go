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

// The publisher replaces no copied bytes: after all scan workers have drained,
// it publishes a new immutable sample in a region the full scan already visited.
type publishAfterScanSource struct {
	*cadenceSource
	verifies       atomic.Int32
	publish        func()
	gap            bool
	localRead      func(context.Context) error
	localFinalWait bool
}

func (s *publishAfterScanSource) Verify(ctx context.Context) error {
	count := s.verifies.Add(1)
	if count == 2 && s.publish != nil {
		s.publish()
	}
	if count == 5 && s.localFinalWait {
		<-ctx.Done()
	}
	return ctx.Err()
}

func TestCacheOffRefreshRequiresSuccessfulFinalProcessVerify(t *testing.T) {
	n, base, e, write := cadenceFixture(t)
	n.Hints = nil
	source := &publishAfterScanSource{cadenceSource: base, localFinalWait: true, publish: func() { write((512<<10)+4096, 1500, nil) }}
	ctx := context.Background()
	sample, err := n.observeInputFrom(ctx, source, e, 0, func() int64 { return 1600 }, time.Now)
	if err == nil || sample.Address != 0 {
		t.Fatalf("child deadline skipped final process verification: sample=%+v err=%v", sample, err)
	}
	if ctx.Err() != nil || n.memorySession().Err() != nil || len(n.Lookups) != 2 || !n.Lookups[1].Stats.LocalStopped {
		t.Fatal("local verify timeout poisoned outer invocation", err, n.Lookups)
	}
}

func (s *publishAfterScanSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if s.localRead != nil && s.verifies.Load() >= 3 {
		if err := s.localRead(ctx); err != nil {
			return 0, err
		}
	}
	if s.gap && address >= 1<<20 {
		return 0, errors.New("fixture gap")
	}
	return s.cadenceSource.Read(ctx, address, b)
}

func TestCacheOffRefreshesSameCallAfterFullDiscoveryPassesPublication(t *testing.T) {
	n, base, e, write := cadenceFixture(t)
	n.Hints = nil
	source := &publishAfterScanSource{cadenceSource: base, publish: func() { write((512<<10)+4096, 1500, nil) }}
	sample, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if err != nil || sample.Address != (512<<10)+4096 || sample.SampleMillis != 1500 {
		t.Fatalf("fresh publication after visited region was lost: sample=%+v err=%v", sample, err)
	}
	if n.Hints != nil || base.regions.Load() != 1 || len(n.Lookups) != 2 || !n.Lookups[0].Coverage.Complete || n.Lookups[1].Coverage.Complete || n.Lookups[1].Path != "nearby_scan" {
		t.Fatal("cache-off refresh lost full discovery, local scope or disabled hints", n.Lookups)
	}
	if n.observationMetrics().Total.LearningBytes != 0 {
		t.Fatal("call-local refresh enabled hint learning")
	}
}

func TestCacheOffRefreshSeedCanAgeThroughFullScan(t *testing.T) {
	n, base, e, write := cadenceFixture(t)
	n.Hints = nil
	source := &publishAfterScanSource{cadenceSource: base, publish: func() { write((512<<10)+4096, 5900, nil) }}
	uptime := func() int64 {
		if source.verifies.Load() < 2 {
			return 1600
		}
		return 6000
	}
	sample, err := n.observeInputFrom(context.Background(), source, e, 0, uptime, time.Now)
	if err != nil || sample.SampleMillis != 5900 || len(n.Lookups) != 2 || !n.Lookups[0].Coverage.Complete {
		t.Fatal("seed aged through scan lost fresh refresh", sample, err, n.Lookups)
	}
}

func TestCacheOffRejectsUnqualifiedOrExpiredRefreshSeeds(t *testing.T) {
	for _, tc := range []struct {
		name      string
		tick, end int64
		alter     func(*InputObservation)
		gap       bool
	}{
		{"oldAtRead", 0, 1600, nil, false},
		{"oldAfterDrain", 1000, 11001, nil, false},
		{"clock", 2000, 1600, nil, false},
		{"slot", 1000, 1600, func(s *InputObservation) { s.NextSlot++ }, false},
		{"actor", 1000, 1600, func(s *InputObservation) { s.GUID = "different" }, false},
		{"runtime", 1000, 1600, func(s *InputObservation) { s.Runtime = "different" }, false},
		{"owner", 1000, 1600, func(s *InputObservation) { s.Owner = "different" }, false},
		{"fence", 1000, 1600, func(s *InputObservation) { s.Fence++ }, false},
		{"build", 1000, 1600, func(s *InputObservation) { s.Build = "different" }, false},
		{"structure", 1000, 1600, func(s *InputObservation) { s.Schema = "different" }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			n, base, e, write := cadenceFixture(t)
			n.Hints = nil
			write(512<<10, tc.tick, tc.alter)
			source := &publishAfterScanSource{cadenceSource: base, gap: tc.gap, publish: func() { write((512<<10)+4096, tc.end-100, nil) }}
			uptime := func() int64 {
				if source.verifies.Load() < 2 {
					return 1600
				}
				return tc.end
			}
			sample, err := n.observeInputFrom(context.Background(), source, e, 0, uptime, time.Now)
			if err == nil || sample.Address != 0 || len(n.Lookups) != 1 || n.Hints != nil {
				t.Fatal("ineligible seed triggered refresh", sample, err, n.Lookups)
			}
		})
	}
}

func TestCacheOffCompletedTraversalWithGapsCanObservePositiveRefresh(t *testing.T) {
	n, base, e, write := cadenceFixture(t)
	n.Hints = nil
	source := &publishAfterScanSource{cadenceSource: base, gap: true, publish: func() { write((512<<10)+4096, 1500, nil) }}
	sample, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if err != nil || sample.SampleMillis != 1500 || len(n.Lookups) != 2 || n.Lookups[0].Coverage.Complete || n.Lookups[0].Coverage.Truncated || len(n.Lookups[0].Coverage.Gaps) == 0 || n.Lookups[1].Coverage.Complete {
		t.Fatal("positive refresh repaired gaps or failed completed traversal", sample, err, n.Lookups)
	}
}

func TestCacheOffRefreshHasNoCrossCallAddressOrLearning(t *testing.T) {
	n, base, e, write := cadenceFixture(t)
	n.Hints = nil
	source := &publishAfterScanSource{cadenceSource: base, publish: func() { write((512<<10)+4096, 1500, nil) }}
	for i := 0; i < 2; i++ {
		sample, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
		if err != nil || sample.Address == 0 {
			t.Fatal(sample, err)
		}
	}
	if base.regions.Load() != 2 || len(n.Lookups) != 3 || n.Lookups[2].Path != "full_scan" || n.Hints != nil || n.observationMetrics().Total.LearningBytes != 0 {
		t.Fatal("refresh became a cross-call cache", n.Lookups)
	}
}

func TestCacheOffRefreshKeepsDeadlineAndSharedBudget(t *testing.T) {
	for _, which := range []string{"localTimeout", "outerCancel", "budget"} {
		t.Run(which, func(t *testing.T) {
			n, base, e, _ := cadenceFixture(t)
			n.Hints = nil
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			source := &publishAfterScanSource{cadenceSource: base}
			switch which {
			case "localTimeout":
				source.localRead = func(ctx context.Context) error { <-ctx.Done(); return ctx.Err() }
			case "outerCancel":
				source.publish = cancel
			case "budget":
				source.publish = func() {
					n.memorySession().Source(base).Read(ctx, 0, make([]byte, 1))
					n.memorySession().Source(base).Read(ctx, 0, make([]byte, 1))
				}
				n.memorySession()
				n.observation.session = memory.NewSession(memory.Budget{MaxReadCalls: 4})
			}
			sample, err := n.observeInputFrom(ctx, source, e, 0, func() int64 { return 1600 }, time.Now)
			if err == nil || sample.Address != 0 || n.Hints != nil {
				t.Fatal("failed refresh authorized input", sample, err)
			}
			if which == "outerCancel" && !errors.Is(err, context.Canceled) {
				t.Fatal(err)
			}
			if which != "localTimeout" && len(n.Lookups) != 1 {
				t.Fatal("failed full traversal attempted refresh", which, n.Lookups)
			}
			if which == "budget" && !errors.Is(err, memory.ErrBudget) {
				t.Fatal(err)
			}
			if which == "localTimeout" && (ctx.Err() != nil || errors.Is(err, memory.ErrBudget) || n.memorySession().Err() != nil || len(n.Lookups) != 2 || !n.Lookups[1].Stats.LocalStopped) {
				t.Fatal("local timeout poisoned invocation", err, n.Lookups)
			}
		})
	}
}
