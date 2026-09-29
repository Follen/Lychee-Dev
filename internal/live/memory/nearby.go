package memory

import (
	"context"
	"errors"
	"sort"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

const nearbyWindowBytes = 1 << 20
const nearbyMaxBytes = 8 << 20
const nearbyMaxReads = 4096

// ErrNearbyBudget means only the bounded neighborhood was attempted. It never
// establishes absence of a record in the process.
var ErrNearbyBudget = errors.New("memory.nearby_budget")

// FindNearby revalidates known-runtime small records and searches near their
// previous addresses. It never enumerates process regions or falls back to a
// whole-process scan. All results and misses have partial coverage.
func FindNearby(ctx context.Context, src Source, selector Selector, hints *Hints) (out LookupResult, err error) {
	started := time.Now()
	out = LookupResult{Path: "nearby_scan", Coverage: Coverage{Truncated: true, Gaps: []Gap{}, Workers: []Worker{}}}
	defer func() {
		out.Coverage.Complete = false
		out.Coverage.Truncated = true
		out.ElapsedMillis = time.Since(started).Milliseconds()
	}()
	if err = ctx.Err(); err != nil {
		return out, err
	}
	if hints == nil || selector.Runtime == ([16]byte{}) || selector.Kind == bridge.MemoryBody || selector.BodyAuthorized {
		out.Fallback = "nearby_ineligible"
		return out, nil
	}
	bounded, cancel := context.WithTimeout(ctx, 250*time.Millisecond)
	defer cancel()
	readCtx, stop := context.WithCancel(bounded)
	defer stop()
	local := &nearbySource{Source: src, cancel: stop}
	if err = src.Verify(readCtx); err != nil {
		return out, err
	}
	defer func() {
		if local.exhausted {
			err = errors.Join(err, ErrNearbyBudget)
		}
	}()
	windows := map[uint64]bool{}
	// Even caller-constructed hints obey the persisted collection limit.
	for i, hint := range hints.Entries {
		if i >= 64 {
			break
		}
		if readCtx.Err() != nil {
			return out, readCtx.Err()
		}
		if !selector.matches(hint.Header) || hint.Header.Kind == bridge.MemoryBody {
			continue
		}
		start := uint64(0)
		if hint.Address > nearbyWindowBytes/2 {
			start = (hint.Address - nearbyWindowBytes/2) &^ uint64(4095)
		}
		if start > ^uint64(0)-nearbyWindowBytes {
			continue
		}
		if len(windows) < 8 {
			windows[start] = true
		}
		record, readErr := ReadRecord(readCtx, local, hint.Address, selector)
		if readErr != nil {
			continue
		}
		if err = src.Verify(readCtx); err != nil {
			return out, err
		}
		out.Path = "cache_hit"
		out.Records = []Record{record}
		hints.learn(record)
		return out, nil
	}
	out.HintMillis = time.Since(started).Milliseconds()
	if err = readCtx.Err(); err != nil {
		return out, err
	}
	for start := range windows {
		local.ranges = append(local.ranges, Range{start, start + nearbyWindowBytes})
	}
	sort.Slice(local.ranges, func(i, j int) bool { return local.ranges[i].Start < local.ranges[j].Start })
	merged := []Range{}
	for _, r := range local.ranges {
		if len(merged) > 0 && r.Start <= merged[len(merged)-1].End {
			if r.End > merged[len(merged)-1].End {
				merged[len(merged)-1].End = r.End
			}
		} else {
			merged = append(merged, r)
		}
	}
	local.ranges = merged
	if len(merged) == 0 {
		out.Fallback = "nearby_no_hints"
		return out, nil
	}
	out.Records, out.Coverage, err = lookup(readCtx, local, selector, Options{Workers: 1, ChunkBytes: 64 << 10, MaxBytes: nearbyMaxBytes}, true, hints)
	for _, record := range out.Records {
		hints.learn(record)
	}
	if len(out.Records) == 0 {
		out.Fallback = "nearby_miss"
	}
	return out, err
}

// These ranges describe search work, not an eligibility assertion. Every read
// still delegates to Source.Read, which checks the live local mappings. The
// wrapper includes point validation, overlap and page salvage in its I/O cap.
type nearbySource struct {
	Source
	ranges    []Range
	bytes     uint64
	reads     int
	exhausted bool
	cancel    context.CancelFunc
}

func (s *nearbySource) Regions(ctx context.Context) ([]Region, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	out := make([]Region, 0, len(s.ranges))
	for _, r := range s.ranges {
		out = append(out, Region{r, true, true, true})
	}
	return out, nil
}
func (s *nearbySource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if s.reads >= nearbyMaxReads || uint64(len(b)) > nearbyMaxBytes-s.bytes {
		s.exhausted = true
		s.cancel()
		return 0, ErrNearbyBudget
	}
	s.reads++
	s.bytes += uint64(len(b))
	return s.Source.Read(ctx, address, b)
}
