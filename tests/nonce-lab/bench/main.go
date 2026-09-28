package main

import (
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	lab "github.com/follenfang/lycheedev/tests/nonce-lab"
	"math/rand"
	"os"
	"runtime"
	"time"
)

type Measurement struct {
	Mode                            string
	Workers, Trial, Passes          int
	Bytes                           uint64
	ElapsedMS                       float64
	GiBPerSecond                    float64
	Candidates, Valid, PayloadBytes uint64
	CoverageComplete                bool
	WorkerStats                     []lab.Worker
}

func main() {
	mib := flag.Int("mib", 256, "resident corpus MiB")
	passes := flag.Int("passes", 50, "full scans per trial")
	trials := flag.Int("trials", 3, "trials")
	out := flag.String("out", "", "JSON output")
	nonce := flag.String("nonce", "f7b2d83a6e19c45d90af317ce856a204", "16-byte hex search nonce")
	copyRead := flag.Bool("copy-read", false, "copy each block into a worker buffer before searching")
	flag.Parse()
	if *mib < 8 || *mib > 1024 || *passes < 1 || *passes > 100 || *trials < 1 || *trials > 5 || *out == "" {
		panic("invalid bounds")
	}
	var id lab.Identity
	n, err := hex.DecodeString(*nonce)
	if err != nil || len(n) != 16 {
		panic("invalid nonce")
	}
	copy(id.Nonce[:], n)
	copy(id.Run[:], "RUN00001")
	copy(id.Ticket[:], "TICKET01")
	id.Epoch = 1
	rng := rand.New(rand.NewSource(20260928))
	regions := []lab.Region{}
	remaining := *mib * 1024 * 1024
	for remaining > 0 {
		size := 8 * 1024 * 1024
		if size > remaining {
			size = remaining
		}
		b := make([]byte, size)
		rng.Read(b)
		// Mix random, zero-filled and ASCII-like pages in the same deterministic corpus.
		for page := 0; page < len(b); page += 4096 {
			end := page + 4096
			if end > len(b) {
				end = len(b)
			}
			if page/4096%3 == 0 {
				clear(b[page:end])
			}
			if page/4096%3 == 1 {
				for j := page; j < end; j++ {
					b[j] = 'a' + b[j]%26
				}
			}
		}
		// Many old valid records share generic magic but not request identity.
		for pos := 8192; pos+4096 < len(b); pos += 65536 {
			old := id
			rng.Read(old.Nonce[:])
			copy(b[pos:], lab.Encode(old, []byte("same-content"), 3))
		}
		regions = append(regions, lab.Region{Data: b, Private: true, Committed: true, Readable: true})
		remaining -= size
	}
	copy(regions[len(regions)-1].Data[1024*1024-15:], lab.Encode(id, make([]byte, 128*1024), 3))
	modes := []struct {
		name        string
		pattern     []byte
		nonce, simd bool
	}{{"generic-stdlib", lab.Magic, false, false}, {"nonce-stdlib", id.Nonce[:], true, false}, {"nonce-avx2", id.Nonce[:], true, true}}
	results := []Measurement{}
	for trial := 1; trial <= *trials; trial++ {
		for k := 0; k < len(modes); k++ {
			mode := modes[(k+trial-1)%len(modes)]
			for _, workers := range []int{1, 8} {
				runtime.GC()
				begun := time.Now()
				m := Measurement{Mode: mode.name, Workers: workers, Trial: trial, Passes: *passes, CoverageComplete: true}
				m.WorkerStats = make([]lab.Worker, workers)
				for pass := 0; pass < *passes; pass++ {
					r := lab.ScanBuffered(regions, mode.pattern, workers, 1024*1024, mode.simd, *copyRead)
					valid, read := lab.VerifyCandidates(regions, r.Hits, id, mode.nonce)
					if len(valid) != 1 {
						panic(fmt.Sprintf("incorrect matches %d", len(valid)))
					}
					m.Candidates += uint64(len(r.Hits))
					m.Valid += uint64(len(valid))
					m.PayloadBytes += uint64(read)
					for w, s := range r.Workers {
						m.Bytes += s.ScannedBytes
						m.CoverageComplete = m.CoverageComplete && s.Complete && !s.Truncated
						a := &m.WorkerStats[w]
						a.PlannedBytes += s.PlannedBytes
						a.ScannedBytes += s.ScannedBytes
						a.Candidates += s.Candidates
						a.Complete = true
					}
				}
				elapsed := time.Since(begun)
				m.ElapsedMS = float64(elapsed.Microseconds()) / 1000
				m.GiBPerSecond = float64(m.Bytes) / (1 << 30) / elapsed.Seconds()
				results = append(results, m)
				fmt.Fprintf(os.Stderr, "%s workers=%d trial=%d %.1f ms %.2f GiB/s\n", m.Mode, workers, trial, m.ElapsedMS, m.GiBPerSecond)
			}
		}
	}
	file, e := os.Create(*out)
	if e != nil {
		panic(e)
	}
	defer file.Close()
	e = json.NewEncoder(file).Encode(map[string]any{"copyRead": *copyRead, "nonce": *nonce, "simdAvailable": lab.SIMDEnabled(), "goVersion": runtime.Version(), "logicalCPUs": runtime.NumCPU(), "residentMiB": *mib, "chunkBytes": 1024 * 1024, "seed": 20260928, "measurements": results, "scope": "resident synthetic buffers; no ReadProcessMemory; generic and nonce modes use the same new record layout, not the historical live scanner"})
	if e != nil {
		panic(e)
	}
}
