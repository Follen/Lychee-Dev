//go:build windows && amd64

// Isolated, read-only experimental process scanner. Never imports input APIs.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	lab "github.com/follenfang/lycheedev/tests/nonce-lab"
	"golang.org/x/sys/windows"
	"os"
	"strings"
	"sync"
	"time"
	"unsafe"
)

type span struct{ Start, End uintptr }
type worker struct {
	PlannedBytes, ScannedBytes     uint64
	Complete, Truncated            bool
	SkippedRegions                 int
	Gaps                           []span
	ReadCalls, Retries, Candidates int
}
type job struct{ Start, End, ReadEnd uintptr }

func readable(p uint32) bool {
	if p&windows.PAGE_GUARD != 0 {
		return false
	}
	switch p & 255 {
	case 2, 4, 8, 32, 64, 128:
		return true
	}
	return false
}
func enumerate(h windows.Handle) ([]span, int, error) {
	out := []span{}
	skipped := 0
	var total uint64
	for address, count := uintptr(0), 0; count < 100000; count++ {
		var info windows.MemoryBasicInformation
		e := windows.VirtualQueryEx(h, address, &info, unsafe.Sizeof(info))
		if e != nil {
			if errors.Is(e, windows.ERROR_INVALID_PARAMETER) && address > 0 {
				return out, skipped, nil
			}
			return nil, skipped, e
		}
		next := info.BaseAddress + info.RegionSize
		if next <= address {
			return nil, skipped, errors.New("region overflow")
		}
		if info.State == windows.MEM_COMMIT && info.Type == 0x20000 && readable(info.Protect) {
			out = append(out, span{info.BaseAddress, next})
			total += uint64(info.RegionSize)
			if total > 32<<30 {
				return nil, skipped, errors.New("32 GiB budget")
			}
		} else {
			skipped++
		}
		address = next
	}
	return nil, skipped, errors.New("region enumeration truncated")
}
func read(h windows.Handle, address uintptr, b []byte) bool {
	if len(b) == 0 {
		return true
	}
	var n uintptr
	e := windows.ReadProcessMemory(h, address, &b[0], uintptr(len(b)), &n)
	return e == nil && n == uintptr(len(b))
}
func main() {
	if e := run(); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
func run() error {
	pid := flag.Uint("pid", 0, "explicit PID")
	created := flag.Uint64("created", 0, "required process creation FILETIME")
	image := flag.String("image", "", "required exact executable path")
	nonce := flag.String("nonce", "", "request nonce hex")
	runHex := flag.String("run", "", "WGC-confirmed run hex")
	mode := flag.String("mode", "simd", "simd or stdlib")
	workers := flag.Int("workers", 8, "1..8")
	out := flag.String("out", "", "new evidence JSON")
	flag.Parse()
	if *pid == 0 || *created == 0 || *image == "" || *out == "" || *workers < 1 || *workers > 8 || (*mode != "simd" && *mode != "stdlib") {
		return errors.New("invalid arguments")
	}
	var id lab.Identity
	n, e := hex.DecodeString(*nonce)
	if e != nil || len(n) != 16 {
		return errors.New("bad nonce")
	}
	copy(id.Nonce[:], n)
	r, e := hex.DecodeString(*runHex)
	if e != nil || len(r) != 8 {
		return errors.New("bad run")
	}
	copy(id.Run[:], r)
	id.Epoch = 1
	copy(id.Ticket[:], "TICKET01")
	h, e := windows.OpenProcess(windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ, false, uint32(*pid))
	if e != nil {
		return e
	}
	defer windows.CloseHandle(h)
	verify := func() error {
		var c, x, k, u windows.Filetime
		if e := windows.GetProcessTimes(h, &c, &x, &k, &u); e != nil {
			return e
		}
		if uint64(c.HighDateTime)<<32|uint64(c.LowDateTime) != *created {
			return errors.New("process identity changed")
		}
		buf := make([]uint16, 32768)
		size := uint32(len(buf))
		if e := windows.QueryFullProcessImageName(h, 0, &buf[0], &size); e != nil {
			return e
		}
		if !strings.EqualFold(windows.UTF16ToString(buf[:size]), *image) {
			return errors.New("executable mismatch")
		}
		var code uint32
		if e := windows.GetExitCodeProcess(h, &code); e != nil {
			return e
		}
		if code != 259 {
			return errors.New("process exited")
		}
		return nil
	}
	if e := verify(); e != nil {
		return e
	}
	start := time.Now()
	regions, skipped, e := enumerate(h)
	if e != nil {
		return e
	}
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	jobs := make(chan job, *workers*2)
	stats := make([]worker, *workers)
	addresses := []uintptr{}
	var mu sync.Mutex
	var wg sync.WaitGroup
	index := bytes.Index
	if *mode == "simd" {
		index = lab.IndexSIMD
	}
	const chunk = 1 << 20
	for w := 0; w < *workers; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			s := worker{Complete: true}
			buffer := make([]byte, chunk+15)
			hits := []uintptr{}
			for j := range jobs {
				s.PlannedBytes += uint64(j.End - j.Start)
				if ctx.Err() != nil {
					s.Complete = false
					s.Truncated = true
					if len(s.Gaps) < 4096 {
						s.Gaps = append(s.Gaps, span{j.Start, j.ReadEnd})
					}
					continue
				}
				b := buffer[:j.ReadEnd-j.Start]
				s.ReadCalls++
				ok := read(h, j.Start, b)
				valid := []span{}
				if ok {
					valid = append(valid, span{0, uintptr(len(b))})
				} else {
					// Two bounded retries, then page-level salvage. Failed pages are never stitched.
					for retry := 0; retry < 2 && !ok; retry++ {
						s.ReadCalls++
						s.Retries++
						ok = read(h, j.Start, b)
					}
					if ok {
						valid = append(valid, span{0, uintptr(len(b))})
					} else {
						for pos := uintptr(0); pos < uintptr(len(b)); {
							end := pos + 4096 - (j.Start+pos)%4096
							if end > uintptr(len(b)) {
								end = uintptr(len(b))
							}
							s.ReadCalls++
							s.Retries++
							if read(h, j.Start+pos, b[pos:end]) {
								if len(valid) > 0 && valid[len(valid)-1].End == pos {
									valid[len(valid)-1].End = end
								} else {
									valid = append(valid, span{pos, end})
								}
							} else {
								s.Complete = false
								if len(s.Gaps) < 4096 {
									s.Gaps = append(s.Gaps, span{j.Start + pos, j.Start + end})
								} else {
									s.Truncated = true
								}
							}
							pos = end
						}
					}
				}
				for _, v := range valid {
					ownedEnd := v.End
					if ownedEnd > j.End-j.Start {
						ownedEnd = j.End - j.Start
					}
					if ownedEnd > v.Start {
						s.ScannedBytes += uint64(ownedEnd - v.Start)
					}
					view := b[v.Start:v.End]
					for pos := 0; pos < len(view); {
						found := index(view[pos:], id.Nonce[:])
						if found < 0 {
							break
						}
						relative := v.Start + uintptr(pos+found)
						if j.Start+relative >= j.End {
							break
						}
						s.Candidates++
						if len(hits) < 4096 {
							hits = append(hits, j.Start+relative)
						} else {
							s.Truncated = true
							s.Complete = false
						}
						pos += found + 1
					}
				}
			}
			mu.Lock()
			stats[w] = s
			addresses = append(addresses, hits...)
			mu.Unlock()
		}(w)
	}
	for _, region := range regions {
		for p := region.Start; p < region.End; {
			end := p + chunk
			if end > region.End {
				end = region.End
			}
			readEnd := end + 15
			if readEnd > region.End {
				readEnd = region.End
			}
			jobs <- job{p, end, readEnd}
			p = end
		}
	}
	close(jobs)
	wg.Wait()
	scanMS := float64(time.Since(start).Microseconds()) / 1000
	records := []map[string]any{}
	seen := map[uintptr]bool{}
	for _, address := range addresses {
		if address < 8 || seen[address] {
			continue
		}
		seen[address] = true
		head := make([]byte, 64)
		if !read(h, address-8, head) {
			continue
		}
		size, e := lab.ValidateHeader(head, id)
		if e != nil {
			continue
		}
		a, b := make([]byte, size+88), make([]byte, size+88)
		if !read(h, address-8, a) || !read(h, address-8, b) || !bytes.Equal(a, b) || lab.Validate(a, id) != nil {
			continue
		}
		sum := sha256.Sum256(a)
		records = append(records, map[string]any{"address": fmt.Sprintf("0x%x", address-8), "payloadBytes": size, "sha256": hex.EncodeToString(sum[:])})
	}
	complete := true
	var planned, scanned uint64
	for _, s := range stats {
		planned += s.PlannedBytes
		scanned += s.ScannedBytes
		complete = complete && s.Complete && !s.Truncated
	}
	identityError := verify()
	if identityError != nil {
		complete = false
	}
	conflict := false
	for i := 1; i < len(records); i++ {
		if records[i]["sha256"] != records[0]["sha256"] {
			conflict = true
		}
	}
	result := map[string]any{"pid": *pid, "processCreated": *created, "image": *image, "nonce": *nonce, "run": *runHex, "mode": *mode, "simdAvailable": lab.SIMDEnabled(), "workers": stats, "skippedRegions": skipped, "plannedBytes": planned, "scannedBytes": scanned, "coverageComplete": complete, "scanMs": scanMS, "totalMs": float64(time.Since(start).Microseconds()) / 1000, "records": records, "conflict": conflict, "recordVerified": len(records) > 0 && !conflict, "identityError": fmt.Sprint(identityError), "coverageScope": "regions enumerated at scan start; not an atomic snapshot; gaps retained after bounded retry", "commitVerified": false}
	file, e := os.OpenFile(*out, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return e
	}
	defer file.Close()
	return json.NewEncoder(file).Encode(result)
}
