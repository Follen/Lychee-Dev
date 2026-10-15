package resource

import (
	"errors"
	"math"
	"sync"
	"testing"
)

func adaptiveTestBudget(t *testing.T, shares int, available *int64) *Budget {
	t.Helper()
	b, err := newAdaptive(DefaultLimits(), shares, func() (int64, error) { return *available, nil })
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestAdaptiveMemoryHandlesLargeTablesAndDecoderScratch(t *testing.T) {
	available := int64(8 << 30)
	b := adaptiveTestBudget(t, 2, &available)
	// Source preparation, SpellName's compact index, then related spell tables.
	for _, cost := range []Cost{
		{RetainedBytes: 96 << 20, MetadataBytes: 96 << 20},
		{RetainedBytes: 38 << 20, MetadataBytes: 38 << 20},
		{RetainedBytes: 250 << 20, MetadataBytes: 151 << 20},
	} {
		if err := b.Charge(cost); err != nil {
			t.Fatal(err)
		}
	}
	if got := b.MemorySnapshot(); got.MetadataBytes < 285<<20 || got.MetadataBytes >= got.CeilingBytes || got.Growths == 0 {
		t.Fatalf("metadata did not grow on demand: %+v", got)
	}
	// A later cold decoder must also grow beyond the old 512 MiB shared cap.
	release, err := b.ReserveScratch(180 << 20)
	if err != nil {
		t.Fatal(err)
	}
	if b.RemainingRetained() != b.MemorySnapshot().RetainedBytes-((384+180)<<20) {
		t.Fatal("remaining includes unapproved capacity")
	}
	release()
	release()
	if err := b.Charge(Cost{RetainedBytes: 200 << 20, MetadataBytes: 160 << 20}); err != nil {
		t.Fatal(err)
	}
	if got := b.Snapshot(); got.RetainedBytes != 584<<20 || got.MetadataBytes != 445<<20 || b.PeakScratchBytes() != 180<<20 {
		t.Fatalf("scratch release or cumulative charges changed: %+v", got)
	}
}

func TestAdaptiveMemoryPressureAndOtherDimensionsRejectAtomically(t *testing.T) {
	available := int64(4 << 30)
	b := adaptiveTestBudget(t, 2, &available)
	if err := b.Charge(Cost{RetainedBytes: 496 << 20, MetadataBytes: 112 << 20}); err != nil {
		t.Fatal(err)
	}
	before, limits := b.Snapshot(), b.MemorySnapshot()
	// Only 8 MiB per query is currently available; the earlier ceiling is not
	// permission to allocate another 32 MiB after other apps consumed memory.
	available = 32 << 20
	for _, cost := range []Cost{
		{RetainedBytes: 32 << 20, MetadataBytes: 32 << 20},
		{MetadataBytes: 32 << 20},
		{RetainedBytes: math.MaxInt64},
		{MetadataBytes: -1},
	} {
		if err := b.Charge(cost); !errors.Is(err, ErrBudget) {
			t.Fatalf("accepted %+v: %v", cost, err)
		}
		if b.Snapshot() != before || b.MemorySnapshot() != limits {
			t.Fatal("rejected charge changed usage or approved limits")
		}
	}
	if _, err := b.ReserveScratch(32 << 20); !errors.Is(err, ErrBudget) || b.PeakScratchBytes() != 0 {
		t.Fatalf("scratch pressure: %v", err)
	}
	available = 4 << 30
	for _, cost := range []Cost{
		{MetadataBytes: 32 << 20, DecodeWork: DefaultLimits().DecodeWork + 1},
		{MetadataBytes: 32 << 20, NetworkBytes: DefaultLimits().NetworkBytes + 1},
		{MetadataBytes: 32 << 20, NetworkRequests: DefaultLimits().NetworkRequests + 1},
	} {
		if err := b.Charge(cost); !errors.Is(err, ErrBudget) || b.Snapshot() != before || b.MemorySnapshot() != limits {
			t.Fatalf("non-memory failure grew budget: %+v %v", cost, err)
		}
	}
	if err := b.Charge(Cost{RetainedBytes: 32 << 20, MetadataBytes: 32 << 20}); err != nil {
		t.Fatal("growth did not recover after headroom returned", err)
	}
}

func TestAdaptiveMemoryDividesConcurrentCapacityAndBoundsLowMemory(t *testing.T) {
	available := int64(512 << 20)
	first, second := adaptiveTestBudget(t, 2, &available), adaptiveTestBudget(t, 2, &available)
	for _, b := range []*Budget{first, second} {
		if b.MemorySnapshot().RetainedBytes != 128<<20 {
			t.Fatal("low-memory query kept the fixed 512 MiB floor")
		}
		if err := b.Charge(Cost{RetainedBytes: 128 << 20, MetadataBytes: 128 << 20}); err != nil {
			t.Fatal(err)
		}
		if err := b.Charge(Cost{MetadataBytes: 1}); !errors.Is(err, ErrBudget) {
			t.Fatal("query exceeded its share", err)
		}
	}
	if first.Snapshot().RetainedBytes+second.Snapshot().RetainedBytes > available/2 {
		t.Fatal("concurrent queries each consumed the whole pool")
	}
	single := adaptiveTestBudget(t, 1, &available)
	if err := single.Charge(Cost{RetainedBytes: 256 << 20, MetadataBytes: 256 << 20}); err != nil {
		t.Fatal("exclusive query did not receive the single-query share", err)
	}
	available = 0
	empty := adaptiveTestBudget(t, 2, &available)
	if _, err := empty.ReserveScratch(1); !errors.Is(err, ErrBudget) {
		t.Fatal("zero headroom became unbounded", err)
	}
}

func TestAdaptiveMemoryConcurrentGrowthAndProbeFailure(t *testing.T) {
	available := int64(4 << 30)
	b := adaptiveTestBudget(t, 2, &available)
	var wg sync.WaitGroup
	for range 8 {
		wg.Go(func() {
			if err := b.Charge(Cost{RetainedBytes: 80 << 20, MetadataBytes: 80 << 20}); err != nil {
				t.Error(err)
			}
		})
	}
	wg.Wait()
	if b.Snapshot().RetainedBytes != 640<<20 || b.Snapshot().MetadataBytes != 640<<20 {
		t.Fatal("concurrent growth lost charges", b.Snapshot())
	}
	failure := errors.New("memory status unavailable")
	if _, err := newAdaptive(DefaultLimits(), 2, func() (int64, error) { return 0, failure }); !errors.Is(err, failure) || !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	before, limits := b.Snapshot(), b.MemorySnapshot()
	b.memory.available = func() (int64, error) { return 0, failure }
	if err := b.Charge(Cost{RetainedBytes: 80 << 20}); !errors.Is(err, failure) || b.Snapshot() != before || b.MemorySnapshot() != limits {
		t.Fatal("failed memory probe changed the budget", err)
	}
}
