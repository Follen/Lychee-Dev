package memory

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"sort"
	"sync"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

type Hint struct {
	Address uint64              `json:"address,string"`
	Header  bridge.MemoryHeader `json:"header"`
}
type Hints struct {
	mu      sync.Mutex
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
		if entry.Header.Kind == bridge.MemoryBody {
			return empty
		}
	}
	return &h
}
func (h *Hints) Save(ctx context.Context, path string) error {
	if h == nil {
		return nil
	}
	h.mu.Lock()
	if len(h.Entries) > 64 {
		h.mu.Unlock()
		return errors.New("memory.hints_limit")
	}
	for _, entry := range h.Entries {
		if entry.Header.Kind == bridge.MemoryBody {
			h.mu.Unlock()
			return errors.New("memory.hints_body")
		}
	}
	b, err := json.Marshal(h)
	h.mu.Unlock()
	if err != nil {
		return err
	}
	if len(b) > 65536 {
		return errors.New("memory.hints_limit")
	}
	return vault.ReplaceFile(ctx, path, b)
}
func (h *Hints) learn(r Record) {
	if h == nil || r.Header.Kind == bridge.MemoryBody {
		return
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.Entries) > 64 {
		h.Entries = append([]Hint(nil), h.Entries[:64]...)
	}
	// One bounded scratch collection avoids repeated growth while retaining
	// the independent destination used during filtering after the stable sort.
	next := make([]Hint, 1, len(h.Entries)+1)
	next[0] = Hint{r.Address, r.Header}
	for _, entry := range h.Entries {
		if entry.Address != r.Address && entry.Header.Kind != bridge.MemoryBody {
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
	h.Entries = h.Entries[:0]
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
	h.mu.Lock()
	defer h.mu.Unlock()
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
	Stats         Stats    `json:"stats"`
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
	return FindWithSession(ctx, src, selector, hints, first, nil)
}

func (h *Hints) entries() []Hint {
	if h == nil {
		return nil
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	n := len(h.Entries)
	if n > 64 {
		n = 64
	}
	entries := make([]Hint, 0, n)
	for _, entry := range h.Entries[:n] {
		if entry.Header.Kind != bridge.MemoryBody {
			entries = append(entries, entry)
		}
	}
	return entries
}

func FindWithSession(ctx context.Context, src Source, selector Selector, hints *Hints, first bool, session *Session) (out LookupResult, err error) {
	src, session = measured(src, session)
	before := session.Stats()
	defer func() { out.Stats = session.Stats().Delta(before) }()
	started := time.Now()
	out = LookupResult{Path: "full_scan"}
	opts := Options{}
	entries := hints.entries()
	if len(entries) > 0 {
		bounded, cancel := context.WithTimeout(ctx, 250*time.Millisecond)
		defer cancel()
		hintCtx := localContext(bounded, ctx)
		if err := src.Verify(ctx); err != nil {
			return out, err
		}
		for _, hint := range entries {
			if hintCtx.Err() != nil {
				break
			}
			base := hint.Address &^ uint64((1<<20)-1)
			opts.Priority = append(opts.Priority, Range{base, base + (1 << 20)})
			// Unknown/new runtime permits region prioritization only, never an
			// old raw-address receipt shortcut to establish runtime continuity.
			if !first || selector.Runtime == ([16]byte{}) || !selector.matches(hint.Header) {
				continue
			}
			record, readErr := ReadRecord(hintCtx, src, hint.Address, selector)
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
	if first && len(entries) > 0 && selector.Runtime != ([16]byte{}) && selector.Kind != bridge.MemoryBody && !selector.BodyAuthorized {
		local, localErr := findNearby(ctx, src, selector, hints, nil, session, false)
		if len(local.Records) > 0 {
			return local, localErr
		}
		if ctx.Err() != nil || session.Err() != nil {
			return local, errors.Join(ctx.Err(), session.Err())
		}
		if localErr != nil && !errors.Is(localErr, ErrNearbyBudget) && !errors.Is(localErr, context.DeadlineExceeded) {
			return local, localErr
		}
		out.Fallback = local.Fallback
	}
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
