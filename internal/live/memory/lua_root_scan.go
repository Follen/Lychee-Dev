package memory

import (
	"bytes"
	"context"
	"errors"
	"fmt"
)

// LuaRootScanError retains coverage even when no root can be selected. The
// message makes a partial search distinguishable from a complete no-match.
type LuaRootScanError struct {
	Evidence LuaRootEvidence
	Cause    error
}

func (e *LuaRootScanError) Error() string {
	return fmt.Sprintf("%v (text scanned=%d/%d complete=%t gaps=%d candidates=%d)", e.Cause, e.Evidence.ScannedBytes, e.Evidence.TextBytes, e.Evidence.ScanComplete, len(e.Evidence.Gaps), e.Evidence.RootCandidates)
}
func (e *LuaRootScanError) Unwrap() error { return e.Cause }

type luaRootRecipe struct {
	id       string
	pattern  []byte
	mask     []bool
	operands []luaRootOperand
}

type luaRootCandidate struct {
	anchor, root uint64
	code         []byte
	recipe       luaRootRecipe
}

// scanLuaRoots never stitches across unknown bytes. A partial ReadProcessMemory
// result is scanned before retrying only its unread suffix at page boundaries.
// The returned candidates are deduplicated by decoded RIP target, not call site.
func scanLuaRoots(r *luaRootModule, layout LuaRootImageLayout, recipes []luaRootRecipe, evidence *LuaRootEvidence) ([]luaRootCandidate, error) {
	text := layout.proof.text
	evidence.TextBytes = text.end - text.start
	ranges := layout.readable
	if ranges == nil {
		ranges = []Range{{Start: text.start, End: text.end}}
	}
	var candidates []luaRootCandidate
	roots := make(map[uint64]bool)
	var tail []byte
	var tailEnd uint64
	maxPattern := 0
	for _, recipe := range recipes {
		maxPattern = max(maxPattern, len(recipe.pattern))
	}
	gap := func(start, end uint64, reason string) {
		if end <= start {
			return
		}
		tail = nil
		n := len(evidence.Gaps)
		if n > 0 && evidence.Gaps[n-1].End == start && evidence.Gaps[n-1].Reason == reason {
			evidence.Gaps[n-1].End = end
		} else {
			evidence.Gaps = append(evidence.Gaps, Gap{Range: Range{Start: start, End: end}, Reason: reason})
		}
	}
	accept := func(at uint64, chunk []byte) error {
		if len(chunk) == 0 {
			return nil
		}
		if at != tailEnd {
			tail = nil
		}
		r.scanned += uint64(len(chunk))
		combined := append(bytes.Clone(tail), chunk...)
		start := at - uint64(len(tail))
		for i := 0; i < len(combined); {
			if err := r.ctx.Err(); err != nil {
				return err
			}
			n := bytes.IndexByte(combined[i:], 0x48)
			if n < 0 {
				break
			}
			i += n
			for _, recipe := range recipes {
				length := len(recipe.pattern)
				if i+length > len(combined) || start+uint64(i+length) <= at || !luaRootMatches(combined[i:i+length], recipe.pattern, recipe.mask) {
					continue
				}
				evidence.PatternMatches++
				if evidence.PatternMatches > 4096 {
					return luaRootError("pattern_match_budget")
				}
				anchor := start + uint64(i)
				code := combined[i : i+length]
				root, err := luaRootRecipeTarget(anchor, code, layout.proof.imageSize, layout.proof.sections, recipe.operands)
				if err != nil {
					evidence.RejectedMatches++
					continue
				}
				if !roots[root] {
					if len(candidates) >= 16 {
						return luaRootError("root_candidate_budget")
					}
					roots[root] = true
					candidates = append(candidates, luaRootCandidate{anchor: anchor, root: root, code: bytes.Clone(code), recipe: recipe})
					evidence.RootCandidates = len(candidates)
				}
			}
			i++
		}
		keep := min(maxPattern-1, len(combined))
		tail = append(tail[:0], combined[len(combined)-keep:]...)
		tailEnd = at + uint64(len(chunk))
		return nil
	}
	buffer := make([]byte, luaRootReadChunk)
	read := func(at, end uint64) (int, error) {
		n, err := r.readSome(at, buffer[:int(end-at)])
		if err != nil || n != int(end-at) {
			evidence.ReadFailures++
		}
		if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) || errors.Is(err, ErrLuaMailboxRootUnsupported) {
			return 0, err
		}
		if e := accept(at, buffer[:n]); e != nil {
			return n, e
		}
		return n, nil
	}
	cursor := text.start
	for _, span := range ranges {
		start, end := max(span.Start, text.start), min(span.End, text.end)
		if start >= end {
			continue
		}
		gap(cursor, start, "unreadable_mapping")
		for at := start; at < end; {
			limit := min(at+luaRootReadChunk, end)
			n, err := read(at, limit)
			if err != nil {
				gap(at, text.end, "scan_stopped")
				return nil, err
			}
			// Only failed large reads get a page-sized recovery pass. Every byte
			// is tried at most twice; no whole-region retry discards a good prefix.
			for retry := at + uint64(n); retry < limit; {
				next := min((retry/4096+1)*4096, limit)
				if n == 0 && retry == at && next == limit {
					gap(retry, next, "module_read_failed")
					break
				}
				got, err := read(retry, next)
				if err != nil {
					gap(retry, text.end, "scan_stopped")
					return nil, err
				}
				gap(retry+uint64(got), next, "module_read_failed")
				retry = next
			}
			at = limit
		}
		cursor = end
	}
	gap(cursor, text.end, "unreadable_mapping")
	evidence.ScanComplete = len(evidence.Gaps) == 0 && r.scanned == evidence.TextBytes
	return candidates, nil
}
