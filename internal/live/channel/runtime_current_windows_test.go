//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func currentRuntimeFixture(t *testing.T, alter func(*InputObservation, *bridge.MemoryHeader)) (*Native, *observationSource, Identity) {
	t.Helper()
	i := Identity{Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Fence: 1, NextSlot: 4, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: "fixture", InputState: bridge.InputSignalCapability}
	blocked := false
	o := InputObservation{Schema: "lycheedev.input.v1", Runtime: i.Runtime, Owner: i.Owner, Fence: i.Fence, NextSlot: i.NextSlot, GUID: i.GUID, Build: i.Build, SampleMillis: 9900, InputBlocked: &blocked}
	runtime, _ := tokenBytes(i.Runtime)
	h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Sequence: 1, Runtime: runtime, Nonce: runtime}
	if alter != nil {
		alter(&o, &h)
	}
	payload, _ := json.Marshal(o)
	raw, err := bridge.EncodeMemoryRecord(h, payload)
	if err != nil {
		t.Fatal(err)
	}
	address := uint64(512 << 10)
	src := &observationSource{data: make([]byte, 2<<20)}
	copy(src.data[address:], raw)
	head, _, err := bridge.DecodeMemoryRecord(raw)
	if err != nil {
		t.Fatal(err)
	}
	n := &Native{Hints: &memory.Hints{Entries: []memory.Hint{{Address: address, Header: head}}}}
	return n, src, i
}

func TestCurrentRuntimeFreshFactAvoidsRegionEnumeration(t *testing.T) {
	for _, tc := range []struct {
		name  string
		alter func(*InputObservation, *bridge.MemoryHeader)
	}{
		{"fresh", nil},
		{"slot_advanced_awaiting_receipt", func(o *InputObservation, _ *bridge.MemoryHeader) { o.NextSlot++ }},
		{"blocked_still_current", func(o *InputObservation, _ *bridge.MemoryHeader) { *o.InputBlocked = true; o.Reason = "input_combat" }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			n, src, i := currentRuntimeFixture(t, tc.alter)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			current, err := n.currentRuntimeFrom(ctx, src, i, func() int64 { return 10000 })
			if err != nil || !current || src.regions != 0 || src.reads != 2 {
				t.Fatal("current input caused broad discovery", current, err, src.regions, src.reads)
			}
		})
	}
}

func TestCurrentRuntimeRejectsUnqualifiedHeapFacts(t *testing.T) {
	for _, tc := range []struct {
		name  string
		alter func(*InputObservation, *bridge.MemoryHeader)
	}{
		{"stale", func(o *InputObservation, _ *bridge.MemoryHeader) { o.SampleMillis = 9499 }},
		{"future", func(o *InputObservation, _ *bridge.MemoryHeader) { o.SampleMillis = 10101 }},
		{"other_runtime", func(o *InputObservation, h *bridge.MemoryHeader) {
			o.Runtime = strings.Repeat("8", 32)
			runtime, _ := tokenBytes(o.Runtime)
			h.Runtime = runtime
			h.Nonce = runtime
		}},
		{"wrong_nonce", func(_ *InputObservation, h *bridge.MemoryHeader) { h.Nonce = [16]byte{9} }},
		{"wrong_ticket", func(_ *InputObservation, h *bridge.MemoryHeader) { h.Ticket = [16]byte{9} }},
		{"wrong_guid", func(o *InputObservation, _ *bridge.MemoryHeader) { o.GUID = "other" }},
		{"wrong_build", func(o *InputObservation, _ *bridge.MemoryHeader) { o.Build = "50504" }},
		{"wrong_owner", func(o *InputObservation, _ *bridge.MemoryHeader) { o.Owner = strings.Repeat("8", 32) }},
		{"wrong_fence", func(o *InputObservation, _ *bridge.MemoryHeader) { o.Fence++ }},
		{"slot_regressed", func(o *InputObservation, _ *bridge.MemoryHeader) { o.NextSlot-- }},
		{"unknown_secret", func(o *InputObservation, _ *bridge.MemoryHeader) { o.InputBlocked = nil }},
		{"sequence_invalid", func(_ *InputObservation, h *bridge.MemoryHeader) { h.Sequence = 0 }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			n, src, i := currentRuntimeFixture(t, tc.alter)
			current, err := n.currentRuntimeFrom(context.Background(), src, i, func() int64 { return 10000 })
			if err != nil || current || src.regions != 0 {
				t.Fatal("unqualified heap fact suppressed fallback", current, err, src.regions)
			}
		})
	}
}

func TestCurrentRuntimeCacheOffAndExpiryDoNotHideDiscovery(t *testing.T) {
	n, src, i := currentRuntimeFixture(t, nil)
	n.Hints = nil
	current, err := n.currentRuntimeFrom(context.Background(), src, i, func() int64 { return 10000 })
	if err != nil || current || src.regions != 0 || src.reads != 0 {
		t.Fatal("cache-off introduced hidden lookup", current, err)
	}
	n, src, i = currentRuntimeFixture(t, func(o *InputObservation, _ *bridge.MemoryHeader) { o.SampleMillis = 9500 })
	var calls atomic.Int32
	current, err = n.currentRuntimeFrom(context.Background(), src, i, func() int64 {
		if calls.Add(1) == 1 {
			return 10000
		}
		return 10001
	})
	if err != nil || current {
		t.Fatal("lookup renewed500ms freshness", current, err)
	}
}

func TestCurrentRuntimeProcessAndCancellationErrorsRemainErrors(t *testing.T) {
	n, src, i := currentRuntimeFixture(t, nil)
	fault := errors.New("fixture process identity changed")
	src.verifyError = fault
	if current, err := n.currentRuntimeFrom(context.Background(), src, i, func() int64 { return 10000 }); current || !errors.Is(err, fault) {
		t.Fatal("process fault interpreted as current", current, err)
	}
	n, src, i = currentRuntimeFixture(t, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if current, err := n.currentRuntimeFrom(ctx, src, i, func() int64 { return 10000 }); current || !errors.Is(err, context.Canceled) || src.reads != 0 {
		t.Fatal("cancelled discovery continued", current, err)
	}
}

type nativeCurrentWaitPeer struct {
	currentRuntimeWaitPeer
	native *Native
	source memory.Source
}

func (p *nativeCurrentWaitPeer) CurrentRuntime(ctx context.Context, i Identity) (bool, error) {
	p.currentChecks++
	return p.native.currentRuntimeFrom(ctx, p.source, i, func() int64 { return 10000 })
}

func TestWaitUsesNativeCurrentFactAndPreservesFullFallback(t *testing.T) {
	for _, mode := range []string{"fresh", "stale", "cache_off"} {
		t.Run(mode, func(t *testing.T) {
			var alter func(*InputObservation, *bridge.MemoryHeader)
			if mode == "stale" {
				alter = func(o *InputObservation, _ *bridge.MemoryHeader) { o.SampleMillis = 9000 }
			}
			n, src, i := currentRuntimeFixture(t, alter)
			if mode == "cache_off" {
				n.Hints = nil
			}
			p := &nativeCurrentWaitPeer{native: n, source: src}
			d, err := New(filepath.Join(t.TempDir(), "connections", "wait.jsonl"), p, i)
			if err != nil {
				t.Fatal(err)
			}
			d.State.Bound = true
			d.State.Owner = i.Owner
			d.State.ID = "CON-" + i.Owner
			if err := d.PrepareOperation(context.Background(), "return {}", 1, "observation"); err != nil {
				t.Fatal(err)
			}
			d.State.Operation.Stage = "running"
			d.State.Operation.PreparedNonce = strings.Repeat("3", 32)
			d.State.Operation.Challenge = strings.Repeat("4", 32)
			ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
			defer cancel()
			p.cancel = cancel
			err = d.Continue(ctx)
			if !errors.Is(err, ErrPending) || p.currentChecks != 1 || (p.discoveries == 0) != (mode == "fresh") {
				t.Fatal("scheduler current/fallback path wrong", mode, err, p.currentChecks, p.discoveries)
			}
			if src.regions != 0 || p.sends != 0 || p.publishes != 0 || d.State.Operation.Stage != "running" {
				t.Fatal("scheduling proof scanned globally or replayed input", mode)
			}
		})
	}
}

func TestCurrentRuntimeFactCannotSuppressReloadDiscovery(t *testing.T) {
	for _, phase := range []string{"intent", "input_attempted", "binding"} {
		p := &currentRuntimeWaitPeer{}
		d := &Driver{Backend: p, State: State{Bound: true, Reload: &ReloadAttempt{Phase: phase}}}
		current, err := d.currentRuntime(context.Background())
		if current || err != nil || p.currentChecks != 0 {
			t.Fatal("old runtime delayed explicit reload", phase, current, err)
		}
	}
}
