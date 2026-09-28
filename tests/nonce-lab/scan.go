package noncelab

import (
	"bytes"
	"sync"
)

// Region is a simulation buffer, not a process address or memory-reading implementation.
type Region struct {
	Data                         []byte
	Private, Committed, Readable bool
}
type Hit struct{ Region, Offset int }
type Worker struct {
	PlannedBytes, ScannedBytes uint64
	Complete, Truncated        bool
	SkippedRegions, Gaps       int
	Candidates                 uint64
}
type Result struct {
	Workers []Worker
	Hits    []Hit
}
type task struct{ region, start, end int }

// Scan uses Go's optimized bytes.Index. No claim of a custom SIMD implementation.
// Ownership by match start removes overlap duplicates. It never stitches separate regions.
func Scan(regions []Region, pattern []byte, workers, chunk int) Result {
	return ScanWith(regions, pattern, workers, chunk, false)
}
func ScanWith(regions []Region, pattern []byte, workers, chunk int, simd bool) Result {
	return ScanBuffered(regions, pattern, workers, chunk, simd, false)
}

// copyRead measures in-process streaming-copy cost, not ReadProcessMemory.
func ScanBuffered(regions []Region, pattern []byte, workers, chunk int, simd, copyRead bool) Result {
	index := bytes.Index
	if simd {
		index = IndexSIMD
	}
	if workers < 1 || workers > 8 || len(pattern) == 0 || chunk < len(pattern) {
		panic("invalid scan bounds")
	}
	result := Result{Workers: make([]Worker, workers)}
	jobs := make(chan task, workers*2)
	var group sync.WaitGroup
	var mu sync.Mutex
	for w := 0; w < workers; w++ {
		group.Add(1)
		go func(w int) {
			defer group.Done()
			stats := Worker{Complete: true}
			hits := []Hit{}
			var buffer []byte
			if copyRead {
				buffer = make([]byte, chunk+len(pattern)-1)
			}
			for j := range jobs {
				data := regions[j.region].Data
				end := j.end + len(pattern) - 1
				if end > len(data) {
					end = len(data)
				}
				stats.PlannedBytes += uint64(j.end - j.start)
				stats.ScannedBytes += uint64(j.end - j.start)
				view := data[j.start:end]
				if copyRead {
					copy(buffer, view)
					view = buffer[:len(view)]
				}
				position := 0
				for position < len(view) {
					found := index(view[position:], pattern)
					if found < 0 {
						break
					}
					offset := position + found
					if j.start+offset >= j.end {
						break
					}
					hits = append(hits, Hit{j.region, j.start + offset})
					stats.Candidates++
					position = offset + 1
				}
			}
			mu.Lock()
			result.Workers[w] = stats
			result.Hits = append(result.Hits, hits...)
			mu.Unlock()
		}(w)
	}
	skipped := 0
	for r, region := range regions {
		if !region.Private || !region.Committed || !region.Readable {
			skipped++
			continue
		}
		for start := 0; start < len(region.Data); start += chunk {
			end := start + chunk
			if end > len(region.Data) {
				end = len(region.Data)
			}
			jobs <- task{r, start, end}
		}
	}
	close(jobs)
	group.Wait()
	result.Workers[0].SkippedRegions = skipped
	return result
}

// VerifyCandidates reads only a fixed header until identity validation succeeds.
func VerifyCandidates(regions []Region, hits []Hit, id Identity, nonceSearch bool) (valid []Hit, payloadBytes int) {
	for _, hit := range hits {
		start := hit.Offset
		if nonceSearch {
			start -= 8
		}
		if start < 0 || hit.Region < 0 || hit.Region >= len(regions) {
			continue
		}
		b := regions[hit.Region].Data
		if start > len(b)-64 {
			continue
		}
		n, e := ValidateHeader(b[start:start+64], id)
		if e != nil || n+88 > len(b)-start {
			continue
		}
		payloadBytes += n
		if Validate(b[start:start+n+88], id) == nil {
			valid = append(valid, Hit{hit.Region, start})
		}
	}
	return
}
