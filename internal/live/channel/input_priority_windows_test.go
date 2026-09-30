//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

// Publish only after the first complete traversal has drained. The original
// copied scan buffers and its 1MiB refresh cannot observe the new location.
type inputPrioritySource struct {
	*cadenceSource
	mu           sync.RWMutex
	fullDeadline time.Time
	fullVerifies atomic.Int32
	generation   atomic.Int32
	firstBulk    sync.Map
	publish      func()
	finalCancel  context.CancelFunc
	finalErr     error
	firstGap     bool
}

func (s *inputPrioritySource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if len(b) >= 1<<20 {
		s.firstBulk.LoadOrStore(s.generation.Load(), address)
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	if s.firstGap && s.fullVerifies.Load() == 1 && address >= 1<<20 && address < 2<<20 {
		return 0, errors.New("first traversal fixture gap")
	}
	return s.cadenceSource.Read(ctx, address, b)
}

func (s *inputPrioritySource) Regions(ctx context.Context) ([]memory.Region, error) {
	s.generation.Add(1)
	return s.cadenceSource.Regions(ctx)
}

func (s *inputPrioritySource) Verify(ctx context.Context) error {
	deadline, bounded := ctx.Deadline()
	if (!bounded && s.fullDeadline.IsZero()) || bounded && deadline.Equal(s.fullDeadline) {
		count := s.fullVerifies.Add(1)
		if count == 2 && s.publish != nil {
			s.mu.Lock()
			s.publish()
			s.mu.Unlock()
		}
		if count == 4 && s.finalCancel != nil {
			s.finalCancel()
		}
		if count == 4 && s.finalErr != nil {
			return s.finalErr
		}
	}
	return ctx.Err()
}

func priorityInputWrite(t *testing.T, source *cadenceSource, e bridge.SlotEnvelope, address int, tick int64, alter func(*InputObservation), headerAlter func(*bridge.MemoryHeader)) {
	t.Helper()
	runtime, _ := tokenBytes(e.Runtime)
	h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime, State: 1, Sequence: 1}
	if headerAlter != nil {
		headerAlter(&h)
	}
	blocked := false
	s := InputObservation{Schema: "lycheedev.input.v1", Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, SampleMillis: tick, InputBlocked: &blocked}
	if alter != nil {
		alter(&s)
	}
	payload, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	data, err := bridge.EncodeMemoryRecord(h, payload)
	if err != nil {
		t.Fatal(err)
	}
	copy(source.data[address:], data)
}

func inputPriorityFixture(t *testing.T, size, seed, fresh int) (*Native, *inputPrioritySource, bridge.SlotEnvelope) {
	t.Helper()
	n, base, e, _ := cadenceFixture(t)
	n.Hints = nil
	base.data = make([]byte, size)
	priorityInputWrite(t, base, e, seed, 1000, nil, nil)
	source := &inputPrioritySource{cadenceSource: base}
	source.publish = func() { priorityInputWrite(t, base, e, fresh, 1500, nil, nil) }
	return n, source, e
}

func TestInputPriorityFindsPublicationOutsideOriginalNeighborhood(t *testing.T) {
	for _, size := range []int{4 << 20, 6 << 20} {
		t.Run(string(rune('0'+size/(1<<20)))+"chunks", func(t *testing.T) {
			seed, fresh := 512<<10, (512<<10)+(3<<20)
			n, source, e := inputPriorityFixture(t, size, seed, fresh)
			s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
			if err != nil || s.SampleMillis != 1500 || s.Address != uint64(fresh) {
				t.Fatalf("new publication beyond 1MiB refresh lost: sample=%+v err=%v regions=%d", s, err, source.regions.Load())
			}
			if source.regions.Load() != 2 || n.Hints != nil || n.observationMetrics().Total.LearningBytes != 0 || len(n.Lookups) != 3 {
				t.Fatal("priority retry reused hints, changed the full traversal count or omitted accounting", n.Lookups)
			}
			first, retry := n.Lookups[0], n.Lookups[2]
			if !first.Coverage.Complete || first.Path != "full_scan" || retry.Path != "full_scan" || retry.Fallback != "call_local_input_priority_retry" || !retry.Coverage.StoppedEarly || retry.Coverage.Complete {
				t.Fatal("full miss and positive retry coverage were confused", n.Lookups)
			}
		})
	}
}

func TestInputPriorityRejectsUnqualifiedSeeds(t *testing.T) {
	for name, alter := range map[string]func(*bridge.MemoryHeader){
		"state":        func(h *bridge.MemoryHeader) { h.State = 2 },
		"zeroSequence": func(h *bridge.MemoryHeader) { h.Sequence = 0 },
		"maxSequence":  func(h *bridge.MemoryHeader) { h.Sequence = ^uint32(0) },
		"ticket":       func(h *bridge.MemoryHeader) { h.Ticket[0] = 1 },
	} {
		t.Run(name, func(t *testing.T) {
			n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
			priorityInputWrite(t, source.cadenceSource, e, 512<<10, 1000, nil, alter)
			s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
			if !errors.Is(err, ErrPending) || s.Address != 0 || source.regions.Load() != 1 {
				t.Fatal("unqualified header supplied priority positions", s, err, source.regions.Load())
			}
		})
	}
	for _, name := range []string{"owner", "guid", "future", "crc", "noSample", "expiredSeed"} {
		t.Run(name, func(t *testing.T) {
			n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
			tick := int64(1600)
			switch name {
			case "owner":
				priorityInputWrite(t, source.cadenceSource, e, 512<<10, 1000, func(s *InputObservation) { s.Owner = "other" }, nil)
			case "guid":
				priorityInputWrite(t, source.cadenceSource, e, 512<<10, 1000, func(s *InputObservation) { s.GUID = "other" }, nil)
			case "future":
				priorityInputWrite(t, source.cadenceSource, e, 512<<10, 1800, nil, nil)
			case "crc":
				source.data[(512<<10)+bridge.MemoryHeaderBytes] ^= 1
			case "noSample":
				clear(source.data)
			case "expiredSeed":
				source.publish = func() {
					tick = 11601
					priorityInputWrite(t, source.cadenceSource, e, (512<<10)+(3<<20), 11500, nil, nil)
				}
			}
			s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return tick }, time.Now)
			if !errors.Is(err, ErrPending) || s.Address != 0 || source.regions.Load() != 1 {
				t.Fatal("invalid or expired seed started extra full lookup", s, err, source.regions.Load())
			}
		})
	}
}

func TestInputPriorityStrictRereadStillRejectsNewPublication(t *testing.T) {
	for _, name := range []string{"owner", "guid", "future", "crc", "after"} {
		t.Run(name, func(t *testing.T) {
			n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
			fresh := (512 << 10) + (3 << 20)
			source.publish = func() {
				priorityInputWrite(t, source.cadenceSource, e, fresh, 1500, func(s *InputObservation) {
					switch name {
					case "owner":
						s.Owner = "other"
					case "guid":
						s.GUID = "other"
					case "future":
						s.SampleMillis = 1800
					}
				}, nil)
				if name == "crc" {
					source.data[fresh+bridge.MemoryHeaderBytes] ^= 1
				}
			}
			after := int64(0)
			if name == "after" {
				after = 1500
			}
			s, err := n.observeInputFrom(context.Background(), source, e, after, func() int64 { return 1600 }, time.Now)
			if !errors.Is(err, ErrPending) || s.Address != 0 || source.regions.Load() != 2 || !n.Lookups[len(n.Lookups)-1].Coverage.Complete {
				t.Fatal("priority ordering weakened the exact selector", s, err, n.Lookups)
			}
		})
	}
}

func TestInputPriorityDoesNotRetainPositionsAcrossObservations(t *testing.T) {
	seed, fresh := (16<<20)+(512<<10), (19<<20)+(512<<10)
	n, source, e := inputPriorityFixture(t, 24<<20, seed, fresh)
	if _, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now); err != nil {
		t.Fatal(err)
	}
	priorityFirst, ok := source.firstBulk.Load(int32(2))
	if !ok || priorityFirst.(uint64) < 12<<20 || priorityFirst.(uint64) >= 20<<20 {
		t.Fatal("call-local range did not affect second traversal order", priorityFirst)
	}
	source.mu.Lock()
	clear(source.data)
	priorityInputWrite(t, source.cadenceSource, e, 512<<10, 1500, nil, nil)
	source.publish = nil
	source.mu.Unlock()
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	first, ok := source.firstBulk.Load(int32(3))
	if err != nil || s.Address != 512<<10 || !ok || first.(uint64) >= 8<<20 || source.regions.Load() != 3 || n.Hints != nil || n.observationMetrics().Total.LearningBytes != 0 {
		t.Fatal("second observation reused previous priority locations", s, err, first, source.regions.Load())
	}
	var reads, bytes uint64
	for _, lookup := range n.Lookups {
		reads += lookup.Stats.ReadCalls
		bytes += lookup.Stats.RequestedBytes
	}
	stats := n.memorySession().Stats()
	if reads != stats.ReadCalls || bytes != stats.RequestedBytes {
		t.Fatal("full and local lookup accounting omitted or duplicated I/O", reads, bytes, stats)
	}
}

func TestInputPriorityMissStillFindsUnprioritizedSuffix(t *testing.T) {
	n, source, e := inputPriorityFixture(t, 14<<20, 512<<10, (12<<20)+4096)
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if err != nil || s.Address != (12<<20)+4096 || source.regions.Load() != 2 || !n.Lookups[len(n.Lookups)-1].Coverage.StoppedEarly {
		t.Fatal("priority range restricted the fresh whole-process fallback", s, err, n.Lookups)
	}
}

func TestInputPriorityPositiveDoesNotRepairFirstTraversalGaps(t *testing.T) {
	n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
	source.firstGap = true
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if err != nil || s.SampleMillis != 1500 || len(n.Lookups) != 3 {
		t.Fatal("completed first traversal gap suppressed independent positive lookup", s, err, n.Lookups)
	}
	first, retry := n.Lookups[0], n.Lookups[2]
	if first.Coverage.Complete || first.Coverage.Truncated || len(first.Coverage.Gaps) == 0 || retry.Coverage.Complete || !retry.Coverage.StoppedEarly {
		t.Fatal("priority positive erased first traversal uncertainty", n.Lookups)
	}
}

func TestInputPriorityNormalDeadlineAndFinalCancellation(t *testing.T) {
	for _, cancelFinal := range []bool{false, true} {
		t.Run(map[bool]string{false: "deadline", true: "finalCancel"}[cancelFinal], func(t *testing.T) {
			n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			source.fullDeadline, _ = ctx.Deadline()
			if cancelFinal {
				source.finalCancel = cancel
			}
			s, err := n.observeInputFrom(ctx, source, e, 0, func() int64 { return 1600 }, time.Now)
			if cancelFinal {
				if !errors.Is(err, context.Canceled) || s.Address != 0 || source.regions.Load() != 2 {
					t.Fatal("final process verification cancellation authorized input", s, err)
				}
			} else if err != nil || s.SampleMillis != 1500 || ctx.Err() != nil || n.memorySession().Err() != nil {
				t.Fatal("normal invocation deadline poisoned priority lookup", s, err)
			}
		})
	}
}

func TestInputPriorityUsesOriginalPhysicalBudget(t *testing.T) {
	n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
	n.memorySession()
	n.observation.session = memory.NewSession(memory.Budget{MaxRequestedBytes: 8 << 20})
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if !errors.Is(err, memory.ErrBudget) || s.Address != 0 || n.memorySession().Stats().RequestedBytes > 8<<20 {
		t.Fatal("priority retry renewed physical credit", s, err, n.memorySession().Stats())
	}
}

func TestInputPriorityFinalProcessChangeRejectsPositive(t *testing.T) {
	n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
	changed := errors.New("fixture process identity changed")
	source.finalErr = changed
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if !errors.Is(err, changed) || s.Address != 0 || source.regions.Load() != 2 {
		t.Fatal("priority positive bypassed final process identity verification", s, err)
	}
}

func TestInputPriorityUsesOriginalLookupCap(t *testing.T) {
	n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
	n.memorySession()
	n.observation.lookups = observationLimit - 2
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
	if err == nil || err.Error() != "live.channel_observation_budget" || s.Address != 0 || source.regions.Load() != 1 || len(n.Lookups) != 2 {
		t.Fatal("priority retry bypassed the invocation lookup cap", s, err, n.Lookups)
	}
}

func TestInputPriorityNeverRetriesCancelledFirstTraversal(t *testing.T) {
	for _, name := range []string{"cancel", "deadline"} {
		t.Run(name, func(t *testing.T) {
			n, source, e := inputPriorityFixture(t, 4<<20, 512<<10, (512<<10)+(3<<20))
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
			defer cancel()
			source.fullDeadline, _ = ctx.Deadline()
			source.publish = func() {
				if name == "cancel" {
					cancel()
				} else {
					<-ctx.Done()
				}
			}
			s, err := n.observeInputFrom(ctx, source, e, 0, func() int64 { return 1600 }, time.Now)
			if err == nil || s.Address != 0 || source.regions.Load() != 1 || len(n.Lookups) != 1 {
				t.Fatal("cancelled first traversal started a priority retry", s, err, n.Lookups)
			}
		})
	}
}
