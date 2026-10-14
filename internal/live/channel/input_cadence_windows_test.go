//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"strings"
	"testing"
	"time"
)

type sampleMailbox struct {
	record memory.Record
	reads  int
}

func (m *sampleMailbox) Binding() memory.LuaRootBinding { return memory.LuaRootBinding{} }
func (m *sampleMailbox) Lookup(ctx context.Context, _ memory.Selector) (memory.LookupResult, error) {
	m.reads++
	return memory.LookupResult{Records: []memory.Record{m.record}}, ctx.Err()
}
func TestMailboxInputCadenceUsesCurrentPublication(t *testing.T) {
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Fence: 1, Index: 3, GUID: "g", Build: "b", Action: "prepare"}
	runtime, _ := tokenBytes(e.Runtime)
	mailbox := &sampleMailbox{}
	n := &Native{Mailbox: mailbox}
	now := time.Unix(100, 0)
	tick := int64(1600)
	write := func(address uint64, sample int64, alter func(*InputObservation)) {
		blocked := false
		s := InputObservation{Schema: InputSchema, Bindings: primaryInputBindings(), Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, SampleMillis: sample, InputBlocked: &blocked}
		if alter != nil {
			alter(&s)
		}
		payload, _ := json.Marshal(s)
		mailbox.record = memory.Record{Address: address, Header: bridge.MemoryHeader{Nonce: runtime, Runtime: runtime, Kind: bridge.MemoryInputState, State: 1, Sequence: 1}, Payload: payload}
	}
	observe := func(after int64) (InputObservation, error) {
		return n.observeInputFrom(context.Background(), e, after, func() int64 { return tick }, func() time.Time { return now })
	}
	write(4096, 1000, nil)
	if _, err := observe(0); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	deadline := n.inputWait.deadline
	now = now.Add(time.Second)
	tick += 1000
	write(8192, tick-600, nil)
	if _, err := observe(0); !errors.Is(err, ErrInputObservationStale) || n.inputWait.deadline != deadline {
		t.Fatal("stale publication renewed grace", err)
	}
	now = now.Add(time.Second)
	if _, err := observe(0); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) {
		t.Fatal("stopped sampler hid runtime discovery", err)
	}
	write(12288, tick, nil)
	s, err := observe(0)
	if err != nil || s.Address != 12288 || n.inputWait.key != "" {
		t.Fatal("current publication not read", s, err)
	}
	if _, err := observe(tick - 50); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal("old sample authorized after input", err)
	}
	write(16384, tick+50, nil)
	if _, err := observe(tick - 50); err != nil {
		t.Fatal("after+100 boundary", err)
	}
	for _, alter := range []func(*InputObservation){func(s *InputObservation) { s.GUID = "other" }, func(s *InputObservation) { s.SampleMillis = tick + 101 }, func(s *InputObservation) { s.Reason = "contradictory" }} {
		n.inputWait.reset()
		write(20480, tick, alter)
		if s, err := observe(0); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) || s.Address != 0 {
			t.Fatal("invalid sample authorized", s, err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := n.observeInputFrom(ctx, e, 0, func() int64 { return tick }, time.Now); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
