package memory

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

type Hint struct {
	Address uint64              `json:"address,string"`
	Header  bridge.MemoryHeader `json:"header"`
}
type Hints struct {
	Schema  string `json:"schema"`
	Scope   string `json:"scope"`
	Entries []Hint `json:"entries"`
}

// Scope includes native process creation identity, executable/build and wire
// version. This is disposable scheduling data, never an ownership record.
func LoadHints(path, scope string) *Hints {
	empty := &Hints{Schema: "lycheedev.memory-hints.v1", Scope: scope}
	f, err := os.Open(path)
	if err != nil {
		return empty
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, 65537))
	if err != nil || len(b) > 65536 {
		return empty
	}
	var h Hints
	if json.Unmarshal(b, &h) != nil || h.Schema != empty.Schema || h.Scope != scope || len(h.Entries) > 64 {
		return empty
	}
	for _, entry := range h.Entries {
		if entry.Address > ^uint64(0)-(bridge.MemoryMaxPayload+120) {
			return empty
		}
	}
	return &h
}
func (h *Hints) Save(ctx context.Context, path string) error {
	if h == nil {
		return nil
	}
	b, err := json.Marshal(h)
	if err != nil {
		return err
	}
	if len(b) > 65536 {
		return errors.New("memory.hints_limit")
	}
	return vault.ReplaceFile(ctx, path, b)
}
func (h *Hints) learn(r Record) {
	if h == nil {
		return
	}
	next := []Hint{{r.Address, r.Header}}
	for _, entry := range h.Entries {
		if entry.Address != r.Address && len(next) < 64 {
			next = append(next, entry)
		}
	}
	h.Entries = next
}

type LookupResult struct {
	Records       []Record `json:"records"`
	Coverage      Coverage `json:"coverage"`
	Path          string   `json:"path"`
	HintMillis    int64    `json:"hintMillis"`
	ElapsedMillis int64    `json:"elapsedMillis"`
	Fallback      string   `json:"fallback,omitempty"`
}

// Find uses the same validator for point reads and scanning. A nil hints value
// is the complete cache-off path. First=false is full audit and never shortcuts.
func Find(ctx context.Context, src Source, selector Selector, hints *Hints, first bool) (LookupResult, error) {
	started := time.Now()
	out := LookupResult{Path: "full_scan"}
	opts := Options{}
	if hints != nil && len(hints.Entries) > 0 {
		if err := src.Verify(ctx); err != nil {
			return out, err
		}
		observed, err := src.Regions(ctx)
		if err != nil {
			return out, err
		}
		ranges, excluded, err := eligible(observed)
		if err != nil {
			return out, err
		}
		var planned uint64
		for _, r := range ranges {
			planned += r.End - r.Start
		}
		for _, hint := range hints.Entries {
			if time.Since(started) > 250*time.Millisecond {
				break
			}
			eligibleAddress := false
			for _, r := range ranges {
				if hint.Address >= r.Start && hint.Address < r.End {
					eligibleAddress = true
					break
				}
			}
			if !eligibleAddress {
				continue
			}
			base := hint.Address &^ uint64((1<<20)-1)
			opts.Priority = append(opts.Priority, Range{base, base + (1 << 20)})
			// Unknown/new runtime permits region prioritization only, never an
			// old raw-address receipt shortcut to establish runtime continuity.
			if !first || selector.Runtime == ([16]byte{}) || !selector.matches(hint.Header) {
				continue
			}
			record, readErr := ReadRecord(ctx, src, hint.Address, selector)
			if readErr != nil {
				continue
			}
			if err = src.Verify(ctx); err != nil {
				return out, err
			}
			out.Path = "cache_hit"
			out.Records = []Record{record}
			out.HintMillis = time.Since(started).Milliseconds()
			out.ElapsedMillis = out.HintMillis
			out.Coverage = Coverage{PlannedBytes: planned, ScannedBytes: 0, Complete: false, Truncated: true, SkippedRegions: excluded, Gaps: []Gap{}, Workers: []Worker{}}
			hints.learn(record)
			return out, nil
		}
		out.Fallback = "hint_miss_or_invalid"
		out.HintMillis = time.Since(started).Milliseconds()
	}
	var err error
	out.Records, out.Coverage, err = Lookup(ctx, src, selector, opts, first)
	for _, record := range out.Records {
		hints.learn(record)
	}
	if len(opts.Priority) > 0 && len(out.Records) > 0 {
		out.Path = "priority_scan"
	}
	out.ElapsedMillis = time.Since(started).Milliseconds()
	return out, err
}
