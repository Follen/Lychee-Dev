package resource

import (
	"errors"
	"math"
	"sync"
	"testing"
)

func TestSharedBudgetRejectsCombinedSourcesAtomically(t *testing.T) {
	b := New(Limits{RetainedBytes: 100, MetadataBytes: 50, DecodeWork: 10, NetworkRequests: 2, NetworkBytes: 100})
	first := Cost{RetainedBytes: 40, MetadataBytes: 20, DecodeWork: 4, NetworkRequests: 1, NetworkBytes: 60}
	if err := b.Charge(first); err != nil {
		t.Fatal(err)
	}
	if err := b.Charge(Cost{RetainedBytes: 40, NetworkRequests: 1, NetworkBytes: 41}); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	if got := b.Snapshot(); got != first {
		t.Fatalf("partial charge: %+v", got)
	}
	if err := b.Charge(Cost{RetainedBytes: 60, MetadataBytes: 30, DecodeWork: 6, NetworkRequests: 1, NetworkBytes: 40}); err != nil {
		t.Fatal(err)
	}
	if err := b.Charge(Cost{DecodeWork: 1}); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	if b.RemainingRetained() != 0 {
		t.Fatal(b.Snapshot())
	}
}

func TestBudgetConcurrentChargeAndOverflow(t *testing.T) {
	b := New(Limits{DecodeWork: 100})
	var wg sync.WaitGroup
	for range 10 {
		wg.Go(func() {
			for range 10 {
				if err := b.Charge(Cost{DecodeWork: 1}); err != nil {
					t.Error(err)
				}
			}
		})
	}
	wg.Wait()
	if b.Snapshot().DecodeWork != 100 {
		t.Fatal(b.Snapshot())
	}
	if err := b.Charge(Cost{DecodeWork: math.MaxInt64}); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	if err := b.Charge(Cost{RetainedBytes: -1}); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	var absent *Budget
	if err := absent.Charge(Cost{RetainedBytes: 1}); err != nil {
		t.Fatal(err)
	}
}

func TestScratchSharesRetainedCapAndReleasesExactlyOnce(t *testing.T) {
	b := New(Limits{RetainedBytes: 100})
	release, err := b.ReserveScratch(60)
	if err != nil {
		t.Fatal(err)
	}
	if b.RemainingRetained() != 40 || b.PeakScratchBytes() != 60 {
		t.Fatal(b.RemainingRetained(), b.PeakScratchBytes())
	}
	if err := b.Charge(Cost{RetainedBytes: 41}); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	if _, err := b.ReserveScratch(41); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	if err := b.Charge(Cost{RetainedBytes: 40}); err != nil {
		t.Fatal(err)
	}
	release()
	release()
	if b.RemainingRetained() != 60 || b.Snapshot().RetainedBytes != 40 {
		t.Fatal(b.RemainingRetained(), b.Snapshot())
	}
	if _, err := b.ReserveScratch(-1); !errors.Is(err, ErrBudget) {
		t.Fatal(err)
	}
	full, err := b.ReserveScratch(60)
	if err != nil {
		t.Fatal(err)
	}
	full()
}
