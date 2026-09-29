package memory

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"sort"
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
		if entry.Address != r.Address {
			next = append(next, entry)
		}
	}
	// Runtime order is only a scheduling preference. Input sequences are
	// comparable within one runtime; retaining its newest eight locations
	// prevents old telemetry copies from consuming every hint/window.
	sort.SliceStable(next, func(i, j int) bool {
		a, b := next[i].Header, next[j].Header
		if a.Runtime != b.Runtime {
			return bytes.Compare(a.Runtime[:], b.Runtime[:]) > 0
		}
		if a.Kind == bridge.MemoryInputState && b.Kind == bridge.MemoryInputState {
			return a.Sequence > b.Sequence
		}
		// Reserve room for up to eight telemetry locations even when many
		// immutable receipts from this runtime remain in the heap.
		return a.Kind == bridge.MemoryInputState && b.Kind != bridge.MemoryInputState
	})
	counts := map[[16]byte]int{}
	h.Entries = nil
	for _, entry := range next {
		if entry.Header.Kind == bridge.MemoryInputState {
			if counts[entry.Header.Runtime] >= 8 {
				continue
			}
			counts[entry.Header.Runtime]++
		}
		h.Entries = append(h.Entries, entry)
		if len(h.Entries) == 64 {
			break
		}
	}
}

// inputHintUseful prefilters scheduling candidates before validating their
// complete bytes. Existing hints remain untrusted and every hit is reread.
func (h *Hints) inputHintUseful(address uint64, header bridge.MemoryHeader) bool {
	count := 0
	minimum := ^uint32(0)
	for _, entry := range h.Entries {
		if entry.Address == address && entry.Header == header {
			return false
		}
		if entry.Header.Kind == bridge.MemoryInputState && entry.Header.Runtime == header.Runtime {
			count++
			if entry.Header.Sequence < minimum {
				minimum = entry.Header.Sequence
			}
		}
	}
	if count >= 8 && header.Sequence <= minimum {
		return false
	}
	if len(h.Entries) >= 64 {
		last := h.Entries[len(h.Entries)-1].Header.Runtime
		if bytes.Compare(header.Runtime[:], last[:]) < 0 {
			return false
		}
	}
	return true
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
		for _, hint := range hints.Entries {
			if time.Since(started) > 250*time.Millisecond {
				break
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
			if err := src.Verify(ctx); err != nil {
				return out, err
			}
			out.Path = "cache_hit"
			out.Records = []Record{record}
			out.HintMillis = time.Since(started).Milliseconds()
			out.ElapsedMillis = out.HintMillis
			out.Coverage = Coverage{Complete: false, Truncated: true, Gaps: []Gap{}, Workers: []Worker{}}
			hints.learn(record)
			return out, nil
		}
		out.Fallback = "hint_miss_or_invalid"
		out.HintMillis = time.Since(started).Milliseconds()
	}
	var err error
	out.Records, out.Coverage, err = lookup(ctx, src, selector, opts, first, hints)
	for _, record := range out.Records {
		hints.learn(record)
	}
	if len(opts.Priority) > 0 && len(out.Records) > 0 {
		out.Path = "priority_scan"
	}
	out.ElapsedMillis = time.Since(started).Milliseconds()
	return out, err
}
