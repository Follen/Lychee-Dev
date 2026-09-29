package memory

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type nearbyMeasuredSource struct {
	*countedHintsSource
	bytes atomic.Uint64
}

func (s *nearbyMeasuredSource) Read(ctx context.Context, at uint64, b []byte) (int, error) {
	s.bytes.Add(uint64(len(b)))
	return s.fakeSource.Read(ctx, at, b)
}
func nearbyFixture(t *testing.T, size, old int) (*nearbyMeasuredSource, Selector, *Hints) {
	t.Helper()
	s := &nearbyMeasuredSource{countedHintsSource: &countedHintsSource{fakeSource: source(size)}}
	selector := hintFixture(t, s.fakeSource, old, "wanted")
	record, err := ReadRecord(context.Background(), s, uint64(old), selector)
	if err != nil {
		t.Fatal(err)
	}
	hints := LoadHints("missing", "scope")
	hints.learn(record)
	s.calls = 0
	s.bytes.Store(0)
	return s, selector, hints
}
func TestFindNearbyFindsMovedRecordWithoutEnumeratingProcess(t *testing.T) {
	for _, next := range []int{(1 << 20) + 40, (1 << 19) + (64 << 10) - 4} {
		s, selector, hints := nearbyFixture(t, 2<<20, 1<<20)
		hintFixture(t, s.fakeSource, next, "wanted")
		selector.Accept = func(r Record) bool { return r.Address == uint64(next) }
		got, err := FindNearby(context.Background(), s, selector, hints)
		if err != nil || len(got.Records) != 1 || got.Records[0].Address != uint64(next) || got.Path != "nearby_scan" || got.Coverage.Complete || !got.Coverage.Truncated || s.regionsCalls.Load() != 0 {
			t.Fatalf("next=%d got=%+v maps=%d err=%v", next, got, s.regionsCalls.Load(), err)
		}
		s.calls = 0
		got, err = FindNearby(context.Background(), s, selector, hints)
		if err != nil || got.Path != "cache_hit" || s.calls != 2 {
			t.Fatalf("warm=%+v reads=%d err=%v", got, s.calls, err)
		}
	}
}
func TestFindNearbyCapsAllReadBytesAndSearchWindows(t *testing.T) {
	s, selector, hints := nearbyFixture(t, 40<<20, 1<<20)
	h := hints.Entries[0].Header
	hints.Entries = nil
	for i := 0; i < 20; i++ {
		hints.Entries = append(hints.Entries, Hint{Address: uint64((i*2 + 1) << 20), Header: h})
	}
	selector.Accept = func(Record) bool { return false }
	got, err := FindNearby(context.Background(), s, selector, hints)
	if !errors.Is(err, ErrNearbyBudget) || s.bytes.Load() > nearbyMaxBytes || s.calls > nearbyMaxReads || s.regionsCalls.Load() != 0 || got.Coverage.PlannedBytes > nearbyMaxBytes || got.Coverage.Complete {
		t.Fatalf("bytes=%d reads=%d maps=%d coverage=%+v err=%v", s.bytes.Load(), s.calls, s.regionsCalls.Load(), got.Coverage, err)
	}
}
func TestFindNearbyCapsDenseCandidatePointReads(t *testing.T) {
	s, selector, hints := nearbyFixture(t, 2<<20, 1<<20)
	for at := 1 << 19; at < (1<<19)+(1<<20); at += 256 {
		hintFixture(t, s.fakeSource, at, "wanted")
	}
	selector.Accept = func(Record) bool { return false }
	got, err := FindNearby(context.Background(), s, selector, hints)
	if !errors.Is(err, ErrNearbyBudget) || s.calls != nearbyMaxReads || s.bytes.Load() > nearbyMaxBytes || got.Coverage.Complete {
		t.Fatalf("reads=%d bytes=%d err=%v", s.calls, s.bytes.Load(), err)
	}
}
func TestFindNearbyRefusesBodyAndUnknownRuntimeAndCacheOff(t *testing.T) {
	s, selector, hints := nearbyFixture(t, 2<<20, 1<<20)
	for _, which := range []string{"body", "authorized", "unknown", "off"} {
		q := selector
		h := hints
		switch which {
		case "body":
			q.Kind = bridge.MemoryBody
		case "authorized":
			q.BodyAuthorized = true
		case "unknown":
			q.Runtime = [16]byte{}
		case "off":
			h = nil
		}
		got, err := FindNearby(context.Background(), s, q, h)
		if err != nil || len(got.Records) != 0 || got.Coverage.Complete || s.calls != 0 || s.regionsCalls.Load() != 0 {
			t.Fatalf("%s got=%+v reads=%d err=%v", which, got, s.calls, err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := FindNearby(ctx, s, selector, hints); !errors.Is(err, context.Canceled) || s.calls != 0 {
		t.Fatalf("cancel err=%v reads=%d", err, s.calls)
	}
}
func TestFindNearbyMissRemainsPartialAndChecksPredicate(t *testing.T) {
	s, selector, hints := nearbyFixture(t, 2<<20, 1<<20)
	selector.Accept = func(Record) bool { return false }
	got, err := FindNearby(context.Background(), s, selector, hints)
	if err != nil || len(got.Records) != 0 || got.Coverage.Complete || !got.Coverage.Truncated || got.Fallback != "nearby_miss" || s.regionsCalls.Load() != 0 {
		t.Fatalf("got=%+v err=%v", got, err)
	}
}

func TestStoppedSalvageGapIsNotUnreadable(t *testing.T) {
	var stopped atomic.Bool
	source := &stopDuringReadSource{fakeSource: source(8192), stop: &stopped}
	worker := Worker{}
	interrupted := false
	spans := readSpan(context.Background(), source, 0, make([]byte, 8192), &worker, func() []Range { t.Fatal("salvage enumerated after stop"); return nil }, func() bool {
		if stopped.Load() {
			interrupted = true
			return true
		}
		return false
	})
	if len(spans) != 0 || !interrupted {
		t.Fatal("stop was not preserved")
	}
	addScanGap(&worker, Range{0, 8192}, context.Background(), interrupted)
	if len(worker.Gaps) != 1 || worker.Gaps[0].Reason != "stopped_early" || !worker.Truncated {
		t.Fatalf("worker=%+v", worker)
	}
	ordinary := Worker{}
	addScanGap(&ordinary, Range{0, 8192}, context.Background(), false)
	if ordinary.Gaps[0].Reason != "unreadable" {
		t.Fatal("ordinary holes lost their reason")
	}
}

type nearbyDeadlineSource struct {
	*countedHintsSource
	callsStarted int
}

func (s *nearbyDeadlineSource) Read(ctx context.Context, _ uint64, _ []byte) (int, error) {
	s.callsStarted++
	<-ctx.Done()
	return 0, ctx.Err()
}
func TestFindNearbySharesDeadlineAcrossPointReadsAndScan(t *testing.T) {
	measured, selector, hints := nearbyFixture(t, 2<<20, 1<<20)
	slow := &nearbyDeadlineSource{countedHintsSource: measured.countedHintsSource}
	got, err := FindNearby(context.Background(), slow, selector, hints)
	if !errors.Is(err, context.DeadlineExceeded) || slow.callsStarted != 1 || slow.regionsCalls.Load() != 0 || got.Coverage.Complete {
		t.Fatalf("reads=%d maps=%d err=%v", slow.callsStarted, slow.regionsCalls.Load(), err)
	}
}
