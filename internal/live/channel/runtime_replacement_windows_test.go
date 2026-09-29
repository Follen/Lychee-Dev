//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"strings"
	"testing"
)

type replacementSource struct {
	data    []byte
	read    func(uint64, []byte) (int, error)
	regions []memory.Region
}

func (s *replacementSource) Verify(ctx context.Context) error { return ctx.Err() }
func (s *replacementSource) find(ctx context.Context, selector memory.Selector, first bool) (memory.LookupResult, error) {
	return memory.Find(ctx, s, selector, nil, first)
}
func (s *replacementSource) Regions(ctx context.Context) ([]memory.Region, error) {
	return s.regions, ctx.Err()
}
func (s *replacementSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if ctx.Err() != nil {
		return 0, ctx.Err()
	}
	if s.read != nil {
		return s.read(address, b)
	}
	if address > uint64(len(s.data)) || uint64(len(b)) > uint64(len(s.data))-address {
		return 0, errors.New("outside")
	}
	return copy(b, s.data[address:]), nil
}
func TestRuntimeReplacementBytesRejectUnsafeSources(t *testing.T) {
	for _, mode := range []string{"valid", "same_sequence_new_address", "historical", "old_runtime", "boundary", "baseline_partial", "after_partial", "checksum", "nonce", "reread_changed", "cancel", "unknown_identity", "contradiction", "unreadable_region", "payload_limit"} {
		t.Run(mode, func(t *testing.T) {
			old, fixture := replacementFixture()
			source := &replacementSource{data: make([]byte, 8192), regions: []memory.Region{{Range: memory.Range{Start: 0, End: 8192}, Private: true, Committed: true, Readable: true}}}
			write := func(address int, o InputObservation, sequence uint32, nonce bool) memory.Record {
				runtime, _ := tokenBytes(o.Runtime)
				h := bridge.MemoryHeader{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: sequence}
				if nonce {
					h.Nonce = [16]byte{9}
				}
				payload, _ := json.Marshal(o)
				raw, e := bridge.EncodeMemoryRecord(h, payload)
				if e != nil {
					t.Fatal(e)
				}
				copy(source.data[address:], raw)
				r, e := memory.ReadRecord(context.Background(), source, uint64(address), memory.Selector{Kind: bridge.MemoryInputState})
				if e != nil {
					t.Fatal(e)
				}
				return r
			}
			o := fixture.Witness.Observation
			seed := write(128, o, 1, false)
			if mode == "historical" {
				write(2048, o, 2, false)
			}
			if mode == "unreadable_region" {
				source.regions[0].Readable = false
			}
			if mode == "baseline_partial" {
				source.read = func(a uint64, b []byte) (int, error) { return len(b) - 1, nil }
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			rounds := 0
			p, err := observeReplacementBytes(ctx, old, []Identity{fixture.Current}, 1, 2, source, []memory.Record{seed}, func(ctx context.Context) error {
				rounds++
				if mode == "reread_changed" && rounds > 1 {
					return nil
				}
				if mode == "cancel" {
					cancel()
					return ctx.Err()
				}
				if mode == "historical" {
					return nil
				}
				if mode == "after_partial" {
					source.read = func(a uint64, b []byte) (int, error) { return len(b) - 1, nil }
					return nil
				}
				next := o
				seq := uint32(2)
				if mode == "old_runtime" {
					next.Runtime = old.Runtime
				}
				if mode == "unknown_identity" {
					next.GUID = "unknown"
				}
				if mode == "payload_limit" {
					next.Reason = strings.Repeat("x", 2200)
				}
				if mode == "same_sequence_new_address" {
					seq = 1
				}
				at := 2048
				if mode == "boundary" {
					at = 8100
					runtime, _ := tokenBytes(next.Runtime)
					payload, _ := json.Marshal(next)
					raw, _ := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: seq}, payload)
					copy(source.data[at:], raw)
					return nil
				}
				write(at, next, seq, mode == "nonce")
				if mode == "checksum" {
					source.data[at+bridge.MemoryHeaderBytes] ^= 1
				}
				if mode == "contradiction" {
					next.Runtime = old.Runtime
					write(4096, next, seq, false)
				}
				if mode == "reread_changed" {
					source.read = func(a uint64, b []byte) (int, error) {
						if len(b) < 8192 {
							source.data[2048+bridge.MemoryHeaderBytes] = '#'
						}
						return copy(b, source.data[a:]), nil
					}
				}
				return nil
			}, source.find)
			if mode == "valid" || mode == "same_sequence_new_address" {
				if err != nil || p == nil || p.Validate(old) != nil {
					t.Fatal(p, err)
				}
				if rounds != 1 {
					t.Fatal(rounds)
				}
			} else if p != nil {
				t.Fatal("unsafe proof", p)
			}
			if mode == "old_runtime" && rounds != 1 {
				t.Fatal("old live runtime did not stop", rounds)
			}
			if mode == "cancel" && !errors.Is(err, context.Canceled) {
				t.Fatal(err)
			}
		})
	}
}
func TestRuntimeReplacementSnapshotBudget(t *testing.T) {
	old, p := replacementFixture()
	_ = old
	s := &replacementSource{data: make([]byte, 10<<20), regions: []memory.Region{{Range: memory.Range{Start: 0, End: 10 << 20}, Private: true, Committed: true, Readable: true}}}
	runtime, _ := tokenBytes(p.Current.Runtime)
	payload, _ := json.Marshal(p.Witness.Observation)
	var seeds []memory.Record
	for i := 0; i < 10; i++ {
		seeds = append(seeds, memory.Record{Address: uint64(i<<20) + 128, Header: bridge.MemoryHeader{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: 1}, Payload: payload})
	}
	spans, e := replacementSnapshots(context.Background(), s, seeds, 8, nil)
	if e != nil {
		t.Fatal(e)
	}
	total := 0
	for _, span := range spans {
		total += len(span.before)
	}
	if len(spans) != 8 || total != 8<<20 {
		t.Fatal(len(spans), total)
	}
}

func TestRuntimeReplacementSnapshotFailedReadsAreBudgeted(t *testing.T) {
	_, p := replacementFixture()
	runtime, _ := tokenBytes(p.Current.Runtime)
	payload, _ := json.Marshal(p.Witness.Observation)
	for _, duplicates := range []bool{false, true} {
		t.Run(fmt.Sprint(duplicates), func(t *testing.T) {
			calls, requested := 0, 0
			s := &replacementSource{regions: []memory.Region{{Range: memory.Range{Start: 0, End: 16 << 20}, Private: true, Committed: true, Readable: true}}}
			s.read = func(_ uint64, b []byte) (int, error) { calls++; requested += len(b); return len(b) - 1, nil }
			var seeds []memory.Record
			for i := 0; i < 4096; i++ {
				address := uint64(i%16) << 20
				if duplicates {
					address = 0
				}
				seeds = append(seeds, memory.Record{Address: address + 128, Header: bridge.MemoryHeader{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: 1}, Payload: payload})
			}
			spans, err := replacementSnapshots(context.Background(), s, seeds, 8, nil)
			if err != nil || len(spans) != 0 || calls > 8 || requested > 8<<20 {
				t.Fatal(len(spans), calls, requested, err)
			}
			if duplicates && calls != 1 {
				t.Fatal("duplicate failed windows reread", calls)
			}
			if !duplicates && calls != 8 {
				t.Fatal(calls)
			}
		})
	}
}

// Lua may allocate its next immutable sample far outside the cached windows.
// Discovery must relocate within this observation, then witness a later write.
func TestRuntimeReplacementRelocatesDistantSamples(t *testing.T) {
	old, fixture := replacementFixture()
	const far = uint64(255 << 20)
	local, distant := make([]byte, 1<<20), make([]byte, 1<<20)
	source := &replacementSource{regions: []memory.Region{
		{Range: memory.Range{Start: 0, End: 1 << 20}, Private: true, Committed: true, Readable: true},
		{Range: memory.Range{Start: far, End: far + 1<<20}, Private: true, Committed: true, Readable: true},
	}}
	source.read = func(address uint64, b []byte) (int, error) {
		data := local
		if address >= far {
			address -= far
			data = distant
		}
		if address >= uint64(len(data)) || uint64(len(b)) > uint64(len(data))-address {
			return 0, errors.New("outside")
		}
		return copy(b, data[address:]), nil
	}
	runtime, _ := tokenBytes(fixture.Current.Runtime)
	payload, _ := json.Marshal(fixture.Witness.Observation)
	write := func(data []byte, offset int, sequence uint32) {
		raw, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Runtime: runtime, Nonce: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: sequence}, payload)
		if err != nil {
			t.Fatal(err)
		}
		copy(data[offset:], raw)
	}
	write(local, 128, 1)
	seed, err := memory.ReadRecord(context.Background(), source, 128, memory.Selector{Kind: bridge.MemoryInputState})
	if err != nil {
		t.Fatal(err)
	}
	// A higher historical sample already inside the observed region must not
	// consume the sole relocation opportunity and hide the distant producer.
	write(local, 2048, 2)
	rounds := 0
	proof, err := observeReplacementBytes(context.Background(), old, []Identity{fixture.Current}, 1, 2, source, []memory.Record{seed}, func(context.Context) error {
		rounds++
		write(distant, rounds*1024, uint32(rounds+1))
		return nil
	}, source.find)
	if err != nil || proof == nil || proof.Validate(old) != nil || proof.Witness.Address < far || rounds > 4 {
		t.Fatalf("distant producer was not relocated promptly: proof=%+v rounds=%d err=%v", proof, rounds, err)
	}
}
