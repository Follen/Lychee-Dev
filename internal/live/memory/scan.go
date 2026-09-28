// Package memory observes a process through a read-only, replaceable adapter.
// Hints affect scheduling only; all callers use the same record validator.
package memory

import (
	"context"
	"errors"
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

type Range struct {
	Start uint64 `json:"start,string"`
	End   uint64 `json:"end,string"`
}

type Region struct {
	Range
	Private   bool
	Committed bool
	Readable  bool
}

type Source interface {
	Verify(context.Context) error
	Regions(context.Context) ([]Region, error)
	Read(context.Context, uint64, []byte) (int, error)
}

type Gap struct {
	Range
	Reason string `json:"reason"`
}

type Worker struct {
	PlannedBytes   uint64 `json:"plannedBytes"`
	ScannedBytes   uint64 `json:"scannedBytes"`
	Complete       bool   `json:"complete"`
	Truncated      bool   `json:"truncated"`
	SkippedRegions int    `json:"skippedRegions"`
	Gaps           []Gap  `json:"gaps"`
	ReadCalls      uint64 `json:"readCalls"`
	Candidates     uint64 `json:"candidates"`
}

type Hit struct {
	Address uint64 `json:"address,string"`
	Pattern int    `json:"pattern"`
}

type Coverage struct {
	StoppedEarly   bool     `json:"stoppedEarly"`
	Reenumerated   bool     `json:"reenumerated"`
	PlannedBytes   uint64   `json:"plannedBytes"`
	ScannedBytes   uint64   `json:"scannedBytes"`
	Complete       bool     `json:"complete"`
	Truncated      bool     `json:"truncated"`
	SkippedRegions int      `json:"skippedRegions"`
	Gaps           []Gap    `json:"gaps"`
	Workers        []Worker `json:"workers"`
	ElapsedMillis  int64    `json:"elapsedMillis"`
}

type Result struct {
	Hits     []Hit    `json:"hits"`
	Coverage Coverage `json:"coverage"`
}

type Options struct {
	Workers    int
	ChunkBytes int
	MaxBytes   uint64
	MaxHits    int
	// Priority is intersected with fresh eligible regions, never used as scope.
	Priority []Range
	Index    func([]byte, []byte) int
	// Visit streams candidates without retaining raw Hits or applying their
	// collection cap. It is concurrent and must bound its own accepted results.
	// Returning true ends a verified lookup, never a full audit. The callback,
	// not an anchor match, owns record validation.
	Visit func(Hit) bool
}

type job struct {
	Range
	readEnd uint64
}

func normalized(opts Options) (Options, error) {
	if opts.Workers == 0 {
		opts.Workers = 8
	}
	if opts.ChunkBytes == 0 {
		opts.ChunkBytes = 1 << 20
	}
	if opts.MaxBytes == 0 {
		opts.MaxBytes = 64 << 30
	}
	if opts.MaxHits == 0 {
		opts.MaxHits = 4096
	}
	if opts.Index == nil {
		opts.Index = IndexSIMD
	}
	if opts.Workers < 1 || opts.Workers > 8 || opts.ChunkBytes < 64 || opts.ChunkBytes > 8<<20 || opts.MaxHits < 1 || opts.MaxHits > 65536 || len(opts.Priority) > 64 {
		return opts, errors.New("memory.invalid_scan_options")
	}
	return opts, nil
}

// eligible coalesces adjacent ranges, allowing boundary anchors without ever
// stitching across inaccessible bytes. Overlapping adapter regions are invalid.
func eligible(regions []Region) ([]Range, int, error) {
	if len(regions) > 100000 {
		return nil, 0, errors.New("memory.region_limit")
	}
	regions = append([]Region(nil), regions...)
	sort.Slice(regions, func(i, j int) bool { return regions[i].Start < regions[j].Start })
	out := []Range{}
	skipped := 0
	var end uint64
	for i, r := range regions {
		if r.End <= r.Start || (i > 0 && r.Start < end) {
			return nil, 0, errors.New("memory.invalid_regions")
		}
		end = r.End
		if !r.Private || !r.Committed || !r.Readable {
			skipped++
			continue
		}
		if len(out) > 0 && out[len(out)-1].End == r.Start {
			out[len(out)-1].End = r.End
		} else {
			out = append(out, r.Range)
		}
	}
	return out, skipped, nil
}

// Scan always enumerates the current process. Short reads are salvaged once at
// page granularity; any remaining hole is retained, including on cancellation.
// It never claims that the observed region set is an atomic heap snapshot.
func Scan(ctx context.Context, src Source, patterns [][]byte, opts Options) (Result, error) {
	started := time.Now()
	result := Result{}
	opts, err := normalized(opts)
	if err != nil {
		return result, err
	}
	maxPattern := 0
	if len(patterns) == 0 || len(patterns) > 16 {
		return result, errors.New("memory.invalid_patterns")
	}
	for _, p := range patterns {
		if len(p) == 0 || len(p) > 4096 {
			return result, errors.New("memory.invalid_patterns")
		}
		if len(p) > maxPattern {
			maxPattern = len(p)
		}
	}
	if err = src.Verify(ctx); err != nil {
		return result, err
	}
	regions, err := src.Regions(ctx)
	if err != nil {
		return result, err
	}
	ranges, skipped, err := eligible(regions)
	if err != nil {
		return result, err
	}
	jobs := []job{}
	for _, r := range ranges {
		if r.End-r.Start > opts.MaxBytes-result.Coverage.PlannedBytes {
			return result, errors.New("memory.byte_budget")
		}
		result.Coverage.PlannedBytes += r.End - r.Start
		for start := r.Start; start < r.End; {
			end := start + uint64(opts.ChunkBytes)
			if end < start || end > r.End {
				end = r.End
			}
			readEnd := end + uint64(maxPattern-1)
			if readEnd < end || readEnd > r.End {
				readEnd = r.End
			}
			jobs = append(jobs, job{Range{start, end}, readEnd})
			start = end
			if len(jobs) > 200000 {
				return result, errors.New("memory.job_limit")
			}
		}
	}
	// Cap the priority advantage at 64 MiB. Cold and warm scans cover identical
	// ranges and share one queue; corrupt hints cannot add or exclude addresses.
	priority := func(j job) bool {
		for _, p := range opts.Priority {
			if p.End > p.Start && j.Start < p.End && p.Start < j.End {
				return true
			}
		}
		return false
	}
	front, rest := []job{}, []job{}
	var priorityBytes uint64
	for _, j := range jobs {
		if priorityBytes < 64<<20 && priority(j) {
			front = append(front, j)
			priorityBytes += j.End - j.Start
		} else {
			rest = append(rest, j)
		}
	}
	jobs = append(front, rest...)
	queue := make(chan job, opts.Workers*2)
	result.Coverage.Workers = make([]Worker, opts.Workers)
	var mu sync.Mutex
	var wg sync.WaitGroup
	var stopped atomic.Bool
	// One fresh map for bounded page salvage. Disappeared ranges remain gaps
	// in the original coverage plan; new mappings do not rewrite that plan.
	var repairOnce sync.Once
	var repairRanges []Range
	refresh := func() []Range {
		repairOnce.Do(func() {
			result.Coverage.Reenumerated = true
			observed, e := src.Regions(ctx)
			if e == nil {
				repairRanges, _, _ = eligible(observed)
			}
		})
		return repairRanges
	}
	seen := map[Hit]bool{}
	for w := 0; w < opts.Workers; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			stats := Worker{Complete: true, Gaps: []Gap{}}
			buffer := make([]byte, opts.ChunkBytes+maxPattern-1)
			for j := range queue {
				stats.PlannedBytes += j.End - j.Start
				if stopped.Load() {
					stats.Truncated = true
					continue
				}
				data := buffer[:int(j.readEnd-j.Start)]
				spans := readSpan(ctx, src, j.Start, data, &stats, refresh)
				var owned []Range
				for _, span := range spans {
					end := span.End
					if end > j.End {
						end = j.End
					}
					if end > span.Start {
						owned = append(owned, Range{span.Start, end})
						stats.ScannedBytes += end - span.Start
					}
					view := data[span.Start-j.Start : span.End-j.Start]
					// Protocol types share one magic; additional independently
					// requested anchors reuse this same streaming read buffer.
					for pattern, p := range patterns {
						for offset := 0; offset+len(p) <= len(view); {
							if stats.Truncated || stopped.Load() || ctx.Err() != nil {
								stats.Truncated = true
								break
							}
							at := opts.Index(view[offset:], p)
							if at < 0 {
								break
							}
							offset += at
							address := span.Start + uint64(offset)
							if address >= j.End {
								break
							}
							stats.Candidates++
							hit := Hit{address, pattern}
							if opts.Visit == nil {
								mu.Lock()
								if !seen[hit] {
									if len(result.Hits) < opts.MaxHits {
										seen[hit] = true
										result.Hits = append(result.Hits, hit)
									} else {
										stats.Truncated = true
									}
								}
								mu.Unlock()
							}
							if opts.Visit != nil && opts.Visit(hit) {
								stopped.Store(true)
								stats.Truncated = true
								break
							}
							offset++
						}
					}
				}
				cursor := j.Start
				for _, span := range owned {
					if span.Start > cursor {
						addGap(&stats, Range{cursor, span.Start}, ctx)
					}
					cursor = span.End
				}
				if cursor < j.End {
					addGap(&stats, Range{cursor, j.End}, ctx)
				}
				// Missing overlap bytes can hide a crossing anchor even when all
				// owned bytes were read. Preserve that uncertainty explicitly.
				cursor = j.End
				for _, span := range spans {
					if span.End <= cursor {
						continue
					}
					if span.Start > cursor {
						addGap(&stats, Range{cursor, span.Start}, ctx)
					}
					cursor = span.End
				}
				if cursor < j.readEnd {
					addGap(&stats, Range{cursor, j.readEnd}, ctx)
				}
			}
			stats.Complete = stats.PlannedBytes == stats.ScannedBytes && !stats.Truncated && len(stats.Gaps) == 0
			result.Coverage.Workers[w] = stats
		}(w)
	}
	for _, j := range jobs {
		queue <- j
	}
	close(queue)
	wg.Wait()
	result.Coverage.SkippedRegions = skipped
	result.Coverage.StoppedEarly = stopped.Load()
	result.Coverage.Gaps = []Gap{}
	for _, w := range result.Coverage.Workers {
		result.Coverage.ScannedBytes += w.ScannedBytes
		result.Coverage.Truncated = result.Coverage.Truncated || w.Truncated
		result.Coverage.Gaps = append(result.Coverage.Gaps, w.Gaps...)
	}
	result.Coverage.Complete = result.Coverage.ScannedBytes == result.Coverage.PlannedBytes && !result.Coverage.Truncated && len(result.Coverage.Gaps) == 0
	result.Coverage.ElapsedMillis = time.Since(started).Milliseconds()
	sort.Slice(result.Hits, func(i, j int) bool { return result.Hits[i].Address < result.Hits[j].Address })
	if err = src.Verify(ctx); err != nil {
		return result, err
	}
	return result, ctx.Err()
}

func addGap(w *Worker, r Range, ctx context.Context) {
	w.Complete = false
	reason := "unreadable"
	if ctx.Err() != nil {
		reason = "cancelled"
		w.Truncated = true
	}
	if len(w.Gaps) < 4096 {
		w.Gaps = append(w.Gaps, Gap{r, reason})
	} else {
		w.Truncated = true
	}
}

func readSpan(ctx context.Context, src Source, start uint64, b []byte, w *Worker, refresh func() []Range) []Range {
	if ctx.Err() != nil {
		return nil
	}
	w.ReadCalls++
	n, _ := src.Read(ctx, start, b)
	if n < 0 || n > len(b) {
		n = 0
	}
	if n == len(b) {
		return []Range{{start, start + uint64(n)}}
	}
	spans := []Range{}
	if n > 0 {
		spans = append(spans, Range{start, start + uint64(n)})
	}
	current := refresh()
	for at := n; at < len(b) && ctx.Err() == nil; {
		end := at + 4096 - int((start+uint64(at))%4096)
		if end > len(b) {
			end = len(b)
		}
		address := start + uint64(at)
		index := sort.Search(len(current), func(i int) bool { return current[i].End > address })
		if index == len(current) || current[index].Start > address || current[index].End < start+uint64(end) {
			at = end
			continue
		}
		w.ReadCalls++
		count, _ := src.Read(ctx, start+uint64(at), b[at:end])
		if count < 0 || count > end-at {
			count = 0
		}
		if count > 0 {
			r := Range{start + uint64(at), start + uint64(at+count)}
			if len(spans) > 0 && spans[len(spans)-1].End == r.Start {
				spans[len(spans)-1].End = r.End
			} else {
				spans = append(spans, r)
			}
		}
		at = end
	}
	return spans
}
