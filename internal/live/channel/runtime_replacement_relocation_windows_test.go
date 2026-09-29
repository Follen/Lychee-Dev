//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func relocationRecord(t *testing.T, address uint64, o InputObservation, sequence uint32) (memory.Record, []byte) {
	t.Helper()
	token, err := tokenBytes(o.Runtime)
	if err != nil {
		t.Fatal(err)
	}
	payload, err := json.Marshal(o)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Runtime: token, Nonce: token, Sequence: sequence}, payload)
	if err != nil {
		t.Fatal(err)
	}
	header, payload, err := bridge.DecodeMemoryRecord(raw)
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Address: address, Header: header, Payload: payload}, raw
}

func TestReplacementRelocationDoesNotAuthorizeHistoricalOrInvalidWrites(t *testing.T) {
	for _, mode := range []string{"historical", "unknown", "old", "checksum"} {
		t.Run(mode, func(t *testing.T) {
			old, fixture := replacementFixture()
			const far = uint64(200 << 20)
			local, distant := make([]byte, 8192), make([]byte, 8192)
			source := &replacementSource{regions: []memory.Region{
				{Range: memory.Range{Start: 0, End: 8192}, Private: true, Committed: true, Readable: true},
				{Range: memory.Range{Start: far, End: far + 8192}, Private: true, Committed: true, Readable: true},
			}}
			source.read = func(address uint64, b []byte) (int, error) {
				data := local
				if address >= far {
					address -= far
					data = distant
				}
				if address > uint64(len(data)) || uint64(len(b)) > uint64(len(data))-address {
					return 0, errors.New("outside region")
				}
				return copy(b, data[address:]), nil
			}
			seed, raw := relocationRecord(t, 128, fixture.Witness.Observation, 1)
			copy(local[128:], raw)
			newer, raw := relocationRecord(t, far+128, fixture.Witness.Observation, 2)
			copy(distant[128:], raw)
			calls, rounds := 0, 0
			proof, err := observeReplacementBytes(context.Background(), old, []Identity{fixture.Current}, 1, 2, source, []memory.Record{seed}, func(context.Context) error {
				rounds++
				if rounds == 3 && mode != "historical" {
					observation := fixture.Witness.Observation
					if mode == "old" {
						observation.Runtime = old.Runtime
					}
					if mode == "unknown" {
						observation.GUID = "unrecognized-character"
					}
					_, bytes := relocationRecord(t, far+2048, observation, 3)
					if mode == "checksum" {
						bytes[bridge.MemoryHeaderBytes] ^= 1
					}
					copy(distant[2048:], bytes)
				}
				return nil
			}, func(_ context.Context, selector memory.Selector, first bool) (memory.LookupResult, error) {
				calls++
				if !first || selector.Accept == nil || selector.Accept(seed) || !selector.Accept(newer) {
					t.Fatal("relocation selector lost bounded preference")
				}
				return memory.LookupResult{Records: []memory.Record{newer}}, nil
			})
			if err != nil || proof != nil || calls != 1 {
				t.Fatalf("invalid relocated proof: %v %v calls=%d", proof, err, calls)
			}
			if (mode == "old" || mode == "unknown") && rounds != 3 {
				t.Fatal("fresh conflicting identity did not stop", rounds)
			}
			if (mode == "historical" || mode == "checksum") && rounds != 14 {
				t.Fatal("round budget changed", rounds)
			}
		})
	}
}

func TestReplacementRelocationSharesDeadlineAndCancellation(t *testing.T) {
	for _, mode := range []string{"deadline", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			old, fixture := replacementFixture()
			source := &replacementSource{data: make([]byte, 8192), regions: []memory.Region{{Range: memory.Range{Start: 0, End: 8192}, Private: true, Committed: true, Readable: true}}}
			seed, raw := relocationRecord(t, 128, fixture.Witness.Observation, 1)
			copy(source.data[128:], raw)
			duration := 30 * time.Second
			if mode == "deadline" {
				duration = 100 * time.Millisecond
			}
			ctx, cancel := context.WithTimeout(context.Background(), duration)
			defer cancel()
			var observedDeadline time.Time
			calls := 0
			proof, err := observeReplacementBytes(ctx, old, []Identity{fixture.Current}, 1, 2, source, []memory.Record{seed}, func(c context.Context) error {
				deadline, ok := c.Deadline()
				if !ok {
					t.Fatal("no observation deadline")
				}
				if observedDeadline.IsZero() {
					observedDeadline = deadline
				} else if deadline != observedDeadline {
					t.Fatal("observation renewed deadline")
				}
				return c.Err()
			}, func(c context.Context, _ memory.Selector, _ bool) (memory.LookupResult, error) {
				calls++
				deadline, ok := c.Deadline()
				if !ok || deadline != observedDeadline {
					t.Fatal("relocation has separate deadline")
				}
				if mode == "cancel" {
					cancel()
				}
				<-c.Done()
				return memory.LookupResult{}, c.Err()
			})
			want := context.DeadlineExceeded
			if mode == "cancel" {
				want = context.Canceled
			}
			if proof != nil || !errors.Is(err, want) || calls != 1 {
				t.Fatal("relocation ignored parent termination", proof, err, calls)
			}
		})
	}
}

func TestReplacementRelocationSnapshotBudgetIncludesShortReads(t *testing.T) {
	old, fixture := replacementFixture()
	source := &replacementSource{regions: []memory.Region{{Range: memory.Range{Start: 0, End: 16 << 20}, Private: true, Committed: true, Readable: true}}}
	var initial, relocated []memory.Record
	for i := 0; i < 8; i++ {
		record, _ := relocationRecord(t, uint64(i<<20)+128, fixture.Witness.Observation, 1)
		initial = append(initial, record)
		record, _ = relocationRecord(t, uint64((i+8)<<20)+128, fixture.Witness.Observation, 2)
		relocated = append(relocated, record)
	}
	calls, requested, discoveries := 0, 0, 0
	source.read = func(_ uint64, b []byte) (int, error) { calls++; requested += len(b); return len(b) - 1, nil }
	proof, err := observeReplacementBytes(context.Background(), old, []Identity{fixture.Current}, 1, 2, source, initial, func(context.Context) error { t.Fatal("no successful baseline to observe"); return nil }, func(_ context.Context, selector memory.Selector, _ bool) (memory.LookupResult, error) {
		discoveries++
		if !selector.Accept(relocated[0]) {
			t.Fatal("new location rejected")
		}
		return memory.LookupResult{Records: relocated}, nil
	})
	if err != nil || proof != nil || discoveries != 1 || calls != 8 || requested != 8<<20 {
		t.Fatalf("snapshot budget: proof=%v err=%v discovery=%d reads=%d bytes=%d", proof, err, discoveries, calls, requested)
	}
}
