package memory

import (
	"bytes"
	"context"
	"errors"
	"sync"
	"testing"
)

type fakeSource struct {
	mu        sync.Mutex
	data      []byte
	regions   []Region
	holes     []Range
	transient bool
	calls     int
}

func (s *fakeSource) Verify(context.Context) error              { return nil }
func (s *fakeSource) Regions(context.Context) ([]Region, error) { return s.regions, nil }
func (s *fakeSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.calls++
	if ctx.Err() != nil {
		return 0, ctx.Err()
	}
	if s.transient && len(b) > 4096 {
		return 0, errors.New("transient block failure")
	}
	end := address + uint64(len(b))
	if end > uint64(len(s.data)) {
		return 0, errors.New("outside")
	}
	for _, h := range s.holes {
		if address < h.End && h.Start < end {
			return 0, errors.New("hole")
		}
	}
	copy(b, s.data[address:end])
	return len(b), nil
}
func source(size int) *fakeSource {
	return &fakeSource{data: make([]byte, size), regions: []Region{{Range{0, uint64(size)}, true, true, true}}}
}

func TestEarlyLookupCoverageIsCompactAndNotComplete(t *testing.T) {
	s := source(1 << 20)
	copy(s.data, []byte("target"))
	r, err := Scan(context.Background(), s, [][]byte{[]byte("target")}, Options{Workers: 1, ChunkBytes: 64, Visit: func(Hit) bool { return true }})
	if err != nil {
		t.Fatal(err)
	}
	c := r.Coverage
	if !c.StoppedEarly || c.Complete || !c.Truncated || c.PlannedBytes != 1<<20 || c.ScannedBytes >= c.PlannedBytes || len(c.Gaps) != 0 {
		t.Fatalf("%+v", c)
	}
	if len(c.Workers) != 1 || c.Workers[0].Complete || len(c.Workers[0].Gaps) != 0 {
		t.Fatal(c.Workers)
	}
}

func TestCrossChunkAndAdjacentRegionAllAlignments(t *testing.T) {
	pattern := []byte("LYCMEM05")
	for at := 56; at < 72; at++ {
		for _, workers := range []int{1, 8} {
			s := source(192)
			s.regions = []Region{{Range{0, 64}, true, true, true}, {Range{64, 192}, true, true, true}}
			copy(s.data[at:], pattern)
			r, err := Scan(context.Background(), s, [][]byte{pattern}, Options{Workers: workers, ChunkBytes: 64, Index: IndexSIMD})
			if err != nil {
				t.Fatal(err)
			}
			if len(r.Hits) != 1 || r.Hits[0].Address != uint64(at) || !r.Coverage.Complete || r.Coverage.ScannedBytes != 192 {
				t.Fatalf("at=%d workers=%d: %+v", at, workers, r)
			}
		}
	}
}
func TestExcludedMappingsAreNotConcatenated(t *testing.T) {
	s := source(256)
	copy(s.data[60:], []byte("LYCMEM05"))
	s.regions = []Region{{Range{0, 64}, true, true, true}, {Range{64, 128}, false, true, true}, {Range{128, 256}, true, true, true}}
	r, err := Scan(context.Background(), s, [][]byte{[]byte("LYCMEM05")}, Options{ChunkBytes: 64})
	if err != nil {
		t.Fatal(err)
	}
	if len(r.Hits) != 0 || r.Coverage.PlannedBytes != 192 || r.Coverage.SkippedRegions != 1 || !r.Coverage.Complete {
		t.Fatalf("%+v", r)
	}
}
func TestGapSalvageAndHonestCoverage(t *testing.T) {
	for _, hole := range []bool{false, true} {
		s := source(16384)
		s.transient = true
		copy(s.data[100:], []byte("target"))
		if hole {
			s.holes = []Range{{4096, 8192}}
		}
		r, err := Scan(context.Background(), s, [][]byte{[]byte("target")}, Options{Workers: 8, ChunkBytes: 16384})
		if err != nil {
			t.Fatal(err)
		}
		if len(r.Hits) != 1 || r.Coverage.Complete == hole {
			t.Fatalf("hole=%v %+v", hole, r)
		}
		want := uint64(16384)
		if hole {
			want -= 4096
		}
		if r.Coverage.ScannedBytes != want {
			t.Fatalf("double-counted coverage: %+v", r.Coverage)
		}
	}
}

type changedMap struct {
	*fakeSource
	enumerations int
}

func (s *changedMap) Regions(context.Context) ([]Region, error) {
	s.enumerations++
	if s.enumerations == 1 {
		return s.regions, nil
	}
	return []Region{{Range{0, 4096}, true, true, true}, {Range{4096, 8192}, false, true, true}, {Range{8192, 16384}, true, true, true}}, nil
}
func TestSupplementReenumeratesAndKeepsDisappearedMappingGap(t *testing.T) {
	s := &changedMap{fakeSource: source(16384)}
	s.transient = true
	copy(s.data[9000:], []byte("target"))
	r, err := Scan(context.Background(), s, [][]byte{[]byte("target")}, Options{ChunkBytes: 16384})
	if err != nil {
		t.Fatal(err)
	}
	if s.enumerations != 2 || !r.Coverage.Reenumerated || r.Coverage.Complete || r.Coverage.ScannedBytes != 12288 || len(r.Hits) != 1 {
		t.Fatalf("maps=%d result=%+v", s.enumerations, r)
	}
	if len(r.Coverage.Gaps) != 1 || r.Coverage.Gaps[0].Start != 4096 || r.Coverage.Gaps[0].End != 8192 {
		t.Fatalf("%+v", r.Coverage.Gaps)
	}
}
func TestCachePriorityCannotRestrictScope(t *testing.T) {
	s := source(256)
	copy(s.data[200:], []byte("actual"))
	r, err := Scan(context.Background(), s, [][]byte{[]byte("actual")}, Options{ChunkBytes: 64, Priority: []Range{{0, 64}, {9999, 10000}, {80, 40}}})
	if err != nil {
		t.Fatal(err)
	}
	if len(r.Hits) != 1 || !r.Coverage.Complete || r.Coverage.ScannedBytes != 256 {
		t.Fatalf("%+v", r)
	}
}
func TestCandidateCapCancellationAndInvalidMaps(t *testing.T) {
	s := source(256)
	copy(s.data, bytes.Repeat([]byte("AA"), 128))
	r, err := Scan(context.Background(), s, [][]byte{[]byte("AA")}, Options{MaxHits: 2, ChunkBytes: 64})
	if err != nil {
		t.Fatal(err)
	}
	if len(r.Hits) != 2 || !r.Coverage.Truncated || r.Coverage.Complete {
		t.Fatalf("%+v", r)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err = Scan(ctx, s, [][]byte{[]byte("AA")}, Options{}); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	s.regions = append(s.regions, Region{Range{10, 20}, true, true, true})
	if _, err = Scan(context.Background(), s, [][]byte{[]byte("AA")}, Options{}); err == nil {
		t.Fatal("overlap accepted")
	}
}
