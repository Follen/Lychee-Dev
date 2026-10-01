//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type cadenceSource struct {
	data    []byte
	regions atomic.Int32
}

func (*cadenceSource) Verify(ctx context.Context) error { return ctx.Err() }
func (s *cadenceSource) Regions(ctx context.Context) ([]memory.Region, error) {
	s.regions.Add(1)
	return []memory.Region{{Range: memory.Range{Start: 0, End: uint64(len(s.data))}, Private: true, Committed: true, Readable: true}}, ctx.Err()
}
func (s *cadenceSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if address > uint64(len(s.data)) || uint64(len(b)) > uint64(len(s.data))-address {
		return 0, errors.New("outside fixture")
	}
	return copy(b, s.data[address:]), nil
}
func cadenceFixture(t *testing.T) (*Native, *cadenceSource, bridge.SlotEnvelope, func(int, int64, func(*InputObservation))) {
	t.Helper()
	e := bridge.SlotEnvelope{Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Fence: 1, Index: 3, GUID: "g", Build: "b", Action: "prepare"}
	runtime, _ := tokenBytes(e.Runtime)
	h := bridge.MemoryHeader{Nonce: runtime, Runtime: runtime, Kind: bridge.MemoryInputState, State: 1}
	source := &cadenceSource{data: make([]byte, 2<<20)}
	write := func(address int, tick int64, alter func(*InputObservation)) {
		blocked := false
		s := InputObservation{Schema: InputSchema, Bindings: primaryInputBindings(), Runtime: e.Runtime, Owner: e.Owner, Fence: e.Fence, NextSlot: e.Index, GUID: e.GUID, Build: e.Build, SampleMillis: tick, InputBlocked: &blocked}
		if alter != nil {
			alter(&s)
		}
		payload, _ := json.Marshal(s)
		h.Sequence++
		data, err := bridge.EncodeMemoryRecord(h, payload)
		if err != nil {
			t.Fatal(err)
		}
		copy(source.data[address:], data)
	}
	write(512<<10, 1000, nil)
	n := &Native{Hints: &memory.Hints{Entries: []memory.Hint{{Address: 512 << 10, Header: h}}}}
	return n, source, e, write
}

func TestCadenceGapWaitsLocallyAndFindsNewAddress(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	now := time.Unix(100, 0)
	tick := int64(1600)
	observe := func() (InputObservation, error) {
		return n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return tick }, func() time.Time { return now })
	}
	if _, err := observe(); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	if source.regions.Load() != 0 {
		t.Fatal("ordinary cadence gap scanned the full process")
	}
	// Lua publishes a different immutable string. Waiting on the old address
	// alone would never find this record.
	now = now.Add(400 * time.Millisecond)
	tick = 2000
	write((512<<10)+4096, 2000, nil)
	s, err := observe()
	if err != nil || s.SampleMillis != 2000 || s.Address != uint64((512<<10)+4096) || source.regions.Load() != 0 {
		t.Fatalf("sample=%+v err=%v regions=%d", s, err, source.regions.Load())
	}
	if n.inputWait.key != "" {
		t.Fatal("fresh sample did not end short wait")
	}
}

func TestCadenceOldRecordsCannotRenewRecoveryGrace(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	now := time.Unix(100, 0)
	tick := int64(1600)
	observe := func() error {
		_, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return tick }, func() time.Time { return now })
		return err
	}
	if err := observe(); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	deadline := n.inputWait.deadline
	for i := 1; i <= 3; i++ {
		now = now.Add(500 * time.Millisecond)
		tick += 500
		write((512<<10)+i*4096, tick-600, nil)
		if err := observe(); !errors.Is(err, ErrInputObservationStale) {
			t.Fatal(err)
		}
		if n.inputWait.deadline != deadline || source.regions.Load() != 0 {
			t.Fatal("old records renewed or bypassed bounded wait")
		}
	}
	now = now.Add(500 * time.Millisecond)
	tick += 500
	if err := observe(); !errors.Is(err, ErrInputObservationStale) || n.inputWait.deadline != deadline {
		t.Fatal("full discovery did not use a recent hint without renewing grace", err)
	}
	// Finding that exact timestamp again grants neither another early stop nor
	// another two seconds. Runtime discovery can proceed even while it is recent.
	if err := observe(); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) {
		t.Fatal("exhausted wait hid runtime loss", err)
	}
	if source.regions.Load() != 1 {
		t.Fatal("full recovery scan did not resume")
	}
}

func TestCadenceRecentFullScanHintIsNotInputAuthority(t *testing.T) {
	for _, age := range []int64{500, 501, 1500, 1501} {
		t.Run(time.Duration(age).String(), func(t *testing.T) {
			n, source, e, _ := cadenceFixture(t)
			n.Hints = nil
			s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1000 + age }, time.Now)
			if age == 500 {
				if err != nil || s.Address == 0 {
					t.Fatalf("fresh boundary: %+v %v", s, err)
				}
				return
			}
			if s.Address != 0 || err == nil {
				t.Fatal("stale record authorized input")
			}
			if errors.Is(err, ErrInputObservationStale) != (age <= 1500) {
				t.Fatalf("age=%d err=%v", age, err)
			}
			if age <= 1500 && n.inputHintMillis != 1000 {
				t.Fatal("missing progress cursor")
			}
		})
	}
}

func TestCadenceRecentHintCannotHideFreshRecordOrRenewWhenStopped(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	n.Hints = nil
	write((512<<10)+4096, 2000, nil)
	observe := func(tick int64) (InputObservation, error) {
		return n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return tick }, time.Now)
	}
	if _, err := observe(2100); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	s, err := observe(2100)
	if err != nil || s.SampleMillis != 2000 {
		t.Fatalf("repeated early hint hid fresh record: %+v %v", s, err)
	}
	if _, err = observe(3501); !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) {
		t.Fatalf("stopped sampler was kept alive: %v", err)
	}
}

func TestCadenceRecentHintExpiresDuringLookup(t *testing.T) {
	n, source, e, _ := cadenceFixture(t)
	n.Hints = nil
	var calls atomic.Int32
	uptime := func() int64 {
		if calls.Add(1) == 1 {
			return 2500
		}
		return 2501
	}
	s, err := n.observeInputFrom(context.Background(), source, e, 0, uptime, time.Now)
	if !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) || s.Address != 0 || n.inputHintMillis != 0 {
		t.Fatalf("expired lookup extended life: %+v %v", s, err)
	}
}

func TestCadenceWrongTargetFutureAndMalformedSamplesDoNotGetGrace(t *testing.T) {
	for _, alter := range []func(*InputObservation){func(s *InputObservation) { s.GUID = "wrong" }, func(s *InputObservation) { s.SampleMillis = 5000 }, func(s *InputObservation) { s.Reason = "contradictory" }} {
		n, source, e, write := cadenceFixture(t)
		write(512<<10, 1000, alter)
		_, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1600 }, time.Now)
		if !errors.Is(err, ErrPending) || errors.Is(err, ErrInputObservationStale) || n.inputWait.key != "" || source.regions.Load() != 1 {
			t.Fatalf("invalid record became cadence proof: %v", err)
		}
	}
}

func TestCadenceCacheOffKeepsCompleteScanner(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	n.Hints = nil
	write(512<<10, 2000, nil)
	s, err := n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 2100 }, time.Now)
	if err != nil || s.SampleMillis != 2000 || source.regions.Load() != 1 {
		t.Fatalf("cache-off changed correctness: %+v %v", s, err)
	}
}

func TestCadenceAfterInputStillRequiresNewSample(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	write(512<<10, 2000, nil)
	now := time.Unix(100, 0)
	if _, err := n.observeInputFrom(context.Background(), source, e, 1950, func() int64 { return 2100 }, func() time.Time { return now }); !errors.Is(err, ErrInputObservationStale) {
		t.Fatal(err)
	}
	write((512<<10)+4096, 2050, nil)
	s, err := n.observeInputFrom(context.Background(), source, e, 1950, func() int64 { return 2100 }, func() time.Time { return now })
	if err != nil || s.SampleMillis != 2050 {
		t.Fatalf("after+100 boundary changed: %+v %v", s, err)
	}
}

func TestCadenceCancellationAndClockRollbackDoNotExtendGrace(t *testing.T) {
	n, source, e, _ := cadenceFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := n.observeInputFrom(ctx, source, e, 0, func() int64 { return 1600 }, time.Now); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	now := time.Unix(100, 0)
	key := inputSampleKey(e, 0)
	if !n.inputWait.stale(key, now) || n.inputWait.stale(key, now.Add(-time.Second)) {
		t.Fatal("rollback extended scheduling grace")
	}
}
