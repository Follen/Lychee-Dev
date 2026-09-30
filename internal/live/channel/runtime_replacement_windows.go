//go:build windows && amd64

package channel

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"sort"
	"strings"
	"time"
)

// ObserveRuntimeReplacement is read-only. Only changed, fully captured bytes
// establish positive production; discovery, clocks, sequence ordering and hints
// merely choose bounded observation locations. No heap-absence inference is made.
func (n *Native) ObserveRuntimeReplacement(ctx context.Context, old Identity) (*RuntimeReplacementProof, error) {
	if n.Guard == nil || n.Process == nil {
		return nil, errors.New("live.channel_guard_required")
	}
	if n.Process.PID != n.Target.ProcessID || n.Process.Created != n.Target.ProcessStartedAt || !strings.EqualFold(n.Process.Image, n.Target.Executable) {
		return nil, errors.New("live.channel_runtime_replacement_target_changed")
	}
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	ctx, stopObservation := n.observationContext(ctx)
	defer stopObservation()
	source := n.memorySource(n.Process)
	if err := n.Guard(ctx); err != nil {
		return nil, err
	}
	if err := source.Verify(ctx); err != nil {
		return nil, err
	}
	current, err := n.currentRuntimeFrom(ctx, source, old, uptimeMillis)
	if err != nil {
		return nil, err
	}
	if current {
		return nil, nil
	}
	candidates, _, err := n.Discover(ctx, "", "")
	if err != nil {
		return nil, err
	}
	var seeds []memory.Record
	if n.Hints != nil {
		for _, h := range n.Hints.Entries {
			if h.Header.Kind != bridge.MemoryInputState {
				continue
			}
			r, e := memory.ReadRecord(ctx, source, h.Address, memory.Selector{Kind: bridge.MemoryInputState})
			if e == nil {
				if _, ok := replacementRecord(r); ok {
					seeds = append(seeds, r)
				}
			}
		}
	}
	if len(seeds) == 0 {
		found, e := n.Find(ctx, memory.Selector{Kind: bridge.MemoryInputState, Accept: func(r memory.Record) bool { _, ok := replacementRecord(r); return ok }}, false)
		if e != nil {
			return nil, e
		}
		seeds = found.Records
	}
	p, err := observeReplacementBytes(ctx, old, candidates, n.Target.ProcessID, n.Target.ProcessStartedAt, source, seeds, func(ctx context.Context) error {
		timer := time.NewTimer(1100 * time.Millisecond)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-timer.C:
			return nil
		}
	}, n.Find)
	if err != nil {
		return nil, err
	}
	if err = source.Verify(ctx); err != nil {
		return nil, err
	}
	if err = n.Guard(ctx); err != nil {
		return nil, err
	}
	return p, nil
}

// CurrentRuntime is a scheduling fact only. A bounded exact current input
// record avoids rediscovering the whole process while an async result or normal
// close is pending. No-cache and unavailable facts preserve full discovery.
func (n *Native) CurrentRuntime(ctx context.Context, identity Identity) (bool, error) {
	if n.Hints == nil || len(n.Hints.Entries) == 0 {
		return false, ctx.Err()
	}
	if n.Guard == nil || n.Process == nil {
		return false, errors.New("live.channel_guard_required")
	}
	if n.Process.PID != n.Target.ProcessID || n.Process.Created != n.Target.ProcessStartedAt || !strings.EqualFold(n.Process.Image, n.Target.Executable) {
		return false, errors.New("live.channel_runtime_replacement_target_changed")
	}
	if err := n.Guard(ctx); err != nil {
		return false, err
	}
	record, err := n.currentRuntimeLookup(ctx, n.Process, identity, uptimeMillis)
	if err != nil || record == nil {
		return false, err
	}
	if err := n.Guard(ctx); err != nil {
		return false, err
	}
	return currentRuntimeRecord(*record, identity, uptimeMillis()), nil
}

func (n *Native) currentRuntimeFrom(ctx context.Context, source memory.Source, identity Identity, uptime func() int64) (bool, error) {
	record, err := n.currentRuntimeLookup(ctx, source, identity, uptime)
	if err != nil || record == nil {
		return false, err
	}
	return currentRuntimeRecord(*record, identity, uptime()), nil
}

func (n *Native) currentRuntimeLookup(ctx context.Context, source memory.Source, identity Identity, uptime func() int64) (*memory.Record, error) {
	if n.Hints == nil || len(n.Hints.Entries) == 0 || !observedInputCapability(identity.InputState) {
		return nil, ctx.Err()
	}
	runtime, err := tokenBytes(identity.Runtime)
	if err != nil {
		return nil, err
	}
	selector := memory.Selector{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime, Accept: func(record memory.Record) bool { return currentRuntimeRecord(record, identity, uptime()) }}
	found, err := n.findPath(ctx, source, selector, true, true)
	if err != nil {
		if localLookupMiss(ctx, err) {
			return nil, nil
		}
		return nil, err
	}
	if len(found.Records) == 0 {
		return nil, nil
	}
	// Lookup verifies the process after an accepted fresh reread. Recheck the
	// 500ms boundary at this return as well; scan time cannot renew old samples.
	return &found.Records[0], nil
}

func currentRuntimeRecord(record memory.Record, identity Identity, now int64) bool {
	observation, valid := replacementRecord(record)
	if !valid || observation.Runtime != identity.Runtime || observation.GUID != identity.GUID || observation.Build != identity.Build || observation.Owner != identity.Owner || observation.Fence != identity.Fence {
		return false
	}
	// The accepted input may have advanced a slot while its exact receipt is
	// still pending. This affects scheduling only; action input still validates
	// its exact immutable startSlot immediately before keys.
	return observation.NextSlot >= identity.NextSlot && observation.NextSlot <= identity.Slots+1 && observation.SampleMillis >= now-500 && observation.SampleMillis <= now+100
}
func replacementRecord(r memory.Record) (InputObservation, bool) {
	var o InputObservation
	h := r.Header
	if len(r.Payload) > 2048 || h.Kind != bridge.MemoryInputState || h.State != 1 || h.Sequence == 0 || h.Sequence == ^uint32(0) || h.Ticket != [16]byte{} || json.Unmarshal(r.Payload, &o) != nil {
		return o, false
	}
	runtime, err := tokenBytes(o.Runtime)
	return o, err == nil && runtime != [16]byte{} && h.Runtime == runtime && h.Nonce == runtime && o.Schema == "lycheedev.input.v1" && validReplacementObservation(o)
}

type replacementSpan struct {
	address uint64
	before  []byte
}

func replacementSnapshots(ctx context.Context, source memory.Source, seeds []memory.Record, limit int, existing []replacementSpan) ([]replacementSpan, error) {
	regions, err := source.Regions(ctx)
	if err != nil {
		return nil, err
	}
	if len(regions) > 100000 {
		return nil, errors.New("memory.region_limit")
	}
	seeds = append([]memory.Record(nil), seeds...)
	sort.SliceStable(seeds, func(i, j int) bool {
		a, _ := replacementRecord(seeds[i])
		b, _ := replacementRecord(seeds[j])
		return a.SampleMillis > b.SampleMillis
	})
	var spans []replacementSpan
	attempted := map[memory.Range]bool{}
	for _, seed := range seeds {
		if len(attempted) >= limit {
			break
		}
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if _, ok := replacementRecord(seed); !ok {
			continue
		}
		duplicate := false
		for _, span := range existing {
			if seed.Address >= span.address && seed.Address < span.address+uint64(len(span.before)) {
				duplicate = true
			}
		}
		for _, span := range spans {
			if seed.Address >= span.address && seed.Address < span.address+uint64(len(span.before)) {
				duplicate = true
			}
		}
		if duplicate {
			continue
		}
		for _, r := range regions {
			if !r.Private || !r.Committed || !r.Readable || r.End <= r.Start || seed.Address < r.Start || seed.Address >= r.End {
				continue
			}
			start := seed.Address &^ uint64((1<<20)-1)
			end := start + (1 << 20)
			if end < start {
				continue
			}
			if start < r.Start {
				start = r.Start
			}
			if end > r.End {
				end = r.End
			}
			window := memory.Range{Start: start, End: end}
			if attempted[window] {
				break
			}
			attempted[window] = true // Failed and short reads consume the same byte budget.
			buf := make([]byte, int(end-start))
			n, e := source.Read(ctx, start, buf)
			if errors.Is(e, memory.ErrBudget) {
				return nil, e
			}
			if e == nil && n == len(buf) {
				spans = append(spans, replacementSpan{start, buf})
			}
			break
		}
	}
	return spans, nil
}
func observeReplacementBytes(ctx context.Context, old Identity, candidates []Identity, pid uint32, creation uint64, source memory.Source, seeds []memory.Record, pause func(context.Context) error, find func(context.Context, memory.Selector, bool) (memory.LookupResult, error)) (*RuntimeReplacementProof, error) {
	if old.Validate() != nil || len(candidates) > 4096 || len(seeds) > 4096 || pid == 0 || creation == 0 {
		return nil, errors.New("live.channel_runtime_replacement_invalid")
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	pending := func(e error) (*RuntimeReplacementProof, error) { return nil, errors.Join(ErrPending, e) }
	if err := source.Verify(ctx); err != nil {
		return nil, err
	}
	// Reserve half the snapshot budget for one relocation. Stale immutable
	// samples remain readable after Lua starts allocating in a distant heap area.
	spans, err := replacementSnapshots(ctx, source, seeds, 4, nil)
	if err != nil {
		return pending(err)
	}
	relocated := false
	for round := 0; round < 14; round++ {
		if !relocated && (round == 2 || len(spans) == 0) {
			relocated = true
			newest := map[[16]byte]uint32{}
			for _, seed := range seeds {
				if _, ok := replacementRecord(seed); ok && seed.Header.Sequence > newest[seed.Header.Runtime] {
					newest[seed.Header.Runtime] = seed.Header.Sequence
				}
			}
			// Sequence is a search preference only. Even a discovered higher
			// sequence may be historical: capture its region before waiting for
			// a subsequent write. No newly located record is itself a witness.
			found, e := find(ctx, memory.Selector{Kind: bridge.MemoryInputState, Accept: func(r memory.Record) bool {
				_, ok := replacementRecord(r)
				if !ok || r.Header.Sequence <= newest[r.Header.Runtime] {
					return false
				}
				for _, span := range spans {
					if r.Address >= span.address && r.Address < span.address+uint64(len(span.before)) {
						return false
					}
				}
				return true
			}}, true)
			if e != nil {
				return pending(e)
			}
			additional, e := replacementSnapshots(ctx, source, found.Records, 4, spans)
			if e != nil {
				return pending(e)
			}
			spans = append(spans, additional...)
			if len(spans) == 0 {
				return nil, nil
			}
		}
		if err = pause(ctx); err != nil {
			return pending(err)
		}
		if err = source.Verify(ctx); err != nil {
			return nil, err
		}
		var selected *RuntimeReplacementProof
		visits, reads := 0, 0
		for _, span := range spans {
			if err = ctx.Err(); err != nil {
				return pending(err)
			}
			buf := make([]byte, len(span.before))
			n, e := source.Read(ctx, span.address, buf)
			if errors.Is(e, memory.ErrBudget) {
				return pending(e)
			}
			if e != nil || n != len(buf) {
				continue
			}
			for offset := 0; offset < len(buf); {
				if err = ctx.Err(); err != nil {
					return pending(err)
				}
				at := bytes.Index(buf[offset:], bridge.MemoryMagic)
				if at < 0 {
					break
				}
				offset += at
				at = offset
				offset++
				visits++
				if visits > 4096 {
					return nil, nil
				}
				if len(buf)-at < bridge.MemoryHeaderBytes {
					continue
				}
				h, e := bridge.DecodeMemoryHeader(buf[at:])
				if e != nil || h.Kind != bridge.MemoryInputState || h.Length > 2048 {
					continue
				}
				length := bridge.MemoryHeaderBytes + int(h.Length) + bridge.MemoryTrailerBytes
				if length > len(buf)-at {
					continue
				}
				raw := buf[at : at+length]
				before := span.before[at : at+length]
				if bytes.Equal(raw, before) {
					continue
				}
				header, payload, e := bridge.DecodeMemoryRecord(raw)
				if e != nil {
					continue
				}
				record := memory.Record{Address: span.address + uint64(at), Header: header, Payload: payload}
				o, ok := replacementRecord(record)
				if !ok {
					continue
				}
				reads++
				if reads > 256 {
					return nil, nil
				}
				fresh, e := memory.ReadRecord(ctx, source, record.Address, memory.Selector{Kind: bridge.MemoryInputState})
				if errors.Is(e, memory.ErrBudget) {
					return pending(e)
				}
				if e != nil || fresh.Header != header || !bytes.Equal(fresh.Payload, payload) {
					continue
				}
				if o.Runtime == old.Runtime {
					return nil, nil
				}
				var match *Identity
				for _, candidate := range candidates {
					if candidate.Validate() == nil && observedInputCapability(candidate.InputState) && candidate.Runtime == o.Runtime && candidate.GUID == o.GUID && candidate.Build == o.Build && candidate.Owner == o.Owner && candidate.Fence == o.Fence && candidate.NextSlot == o.NextSlot && candidate.Build == old.Build && candidate.Product == old.Product && candidate.Release == old.Release {
						v := candidate
						match = &v
						break
					}
				}
				// Unknown freshly produced identities are conflicting evidence, never ignored.
				if match == nil {
					return nil, nil
				}
				if selected != nil && selected.Current != *match {
					return nil, nil
				}
				a, z := sha256.Sum256(before), sha256.Sum256(raw)
				p := &RuntimeReplacementProof{Schema: RuntimeReplacementProofSchema, ProcessID: pid, ProcessStartedAt: creation, Current: *match, Witness: RuntimeReplacementWitness{Address: record.Address, Length: uint32(length), BeforeSHA256: hex.EncodeToString(a[:]), AfterSHA256: hex.EncodeToString(z[:]), Sequence: header.Sequence, Observation: o}}
				if p.Validate(old) != nil {
					return nil, nil
				}
				selected = p
			}
		}
		if selected != nil {
			if err = source.Verify(ctx); err != nil {
				return nil, err
			}
			return selected, nil
		}
	}
	return nil, nil
}
