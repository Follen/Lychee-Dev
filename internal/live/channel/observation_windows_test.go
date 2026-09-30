//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"hash/adler32"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

type observationSource struct {
	mu             sync.Mutex
	data           []byte
	reads, regions int
	beforeRead     func(uint64, []byte) (int, error, bool)
	afterRead      func(uint64, []byte)
	verifyError    error
}

func (s *observationSource) Verify(ctx context.Context) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return errors.Join(ctx.Err(), s.verifyError)
}
func (s *observationSource) Regions(ctx context.Context) ([]memory.Region, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.regions++
	return []memory.Region{{Range: memory.Range{End: uint64(len(s.data))}, Private: true, Committed: true, Readable: true}}, ctx.Err()
}
func (s *observationSource) Read(ctx context.Context, addr uint64, out []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.reads++
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if s.beforeRead != nil {
		if count, err, stop := s.beforeRead(addr, out); stop {
			return count, err
		}
	}
	if addr > uint64(len(s.data)) || uint64(len(out)) > uint64(len(s.data))-addr {
		return 0, errors.New("fixture mapping unavailable")
	}
	count := copy(out, s.data[addr:])
	if s.afterRead != nil {
		s.afterRead(addr, out)
	}
	return count, ctx.Err()
}

type observationReader struct {
	native *Native
	source memory.Source
}

func (r observationReader) Find(ctx context.Context, s memory.Selector, first bool) (memory.LookupResult, error) {
	return r.native.find(ctx, r.source, s, first)
}
func (r observationReader) findAuthorizedBody(ctx context.Context, h memory.Record, hs, bs memory.Selector) (memory.LookupResult, error) {
	return r.native.findAuthorizedBodyFrom(ctx, r.source, h, hs, bs)
}

func observationFixture(t *testing.T, bodyAddress uint64, cache bool) (*Native, *observationSource, ObservationQuery, uint64, uint64) {
	t.Helper()
	identity := Identity{Schema: bridge.SlotSchema, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Fence: 1, Slots: 200, NextSlot: 4, Character: "c", Realm: "r", GUID: "g", Build: "120100", Product: "retail", Release: "fixture"}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Runtime: identity.Runtime, Owner: identity.Owner, Fence: 1, Index: 2, GUID: "g", Build: identity.Build, Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Action: "prepare"}
	payload := []byte(`{"ok":false,"message":"business failure remains a complete report"}`)
	receipt := Receipt{Identity: identity, Nonce: e.Nonce, Ticket: e.Ticket, Action: "prepare", State: "reported", ReportBytes: uint32(len(payload)), ReportChecksum: adler32.Checksum(payload)}
	jsonHead, _ := json.Marshal(receipt)
	nonce, _ := tokenBytes(e.Nonce)
	runtime, _ := tokenBytes(e.Runtime)
	ticket, _ := tokenBytes(e.Ticket)
	headRaw, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 3, Nonce: nonce, Runtime: runtime, Ticket: ticket, Sequence: 1}, jsonHead)
	if err != nil {
		t.Fatal(err)
	}
	bodyRaw, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: bridge.MemoryBody, State: 3, Nonce: nonce, Runtime: runtime, Ticket: ticket, Sequence: 1}, payload)
	if err != nil {
		t.Fatal(err)
	}
	headAddress := uint64(512 << 10)
	source := &observationSource{data: make([]byte, 4<<20)}
	copy(source.data[headAddress:], headRaw)
	copy(source.data[bodyAddress:], bodyRaw)
	native := &Native{Target: desktop.WindowIdentity{ProcessID: 7, ProcessStartedAt: 132000000000000000, Executable: "fixture.exe"}}
	if cache {
		header, _, _ := bridge.DecodeMemoryRecord(headRaw)
		native.Hints = &memory.Hints{Entries: []memory.Hint{{Address: headAddress, Header: header}}}
	}
	return native, source, ObservationQuery{Kind: "result", Envelope: e, Identity: identity}, headAddress, bodyAddress
}

func TestObservationAuthorizedBodyNearbyAndFullFallback(t *testing.T) {
	for _, tc := range []struct {
		name    string
		body    uint64
		cache   bool
		regions bool
	}{{"near", 544 << 10, true, false}, {"far", 3 << 20, true, true}, {"cache_off", 544 << 10, false, true}} {
		t.Run(tc.name, func(t *testing.T) {
			n, src, q, _, _ := observationFixture(t, tc.body, tc.cache)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			got, err := ObserveRecords(ctx, observationReader{n, src}, q)
			if err != nil || !json.Valid(got.Payload) || !strings.Contains(string(got.Payload), `"ok":false`) {
				t.Fatal(got, err)
			}
			if (src.regions > 0) != tc.regions {
				t.Fatal("unexpected full-process discovery", src.regions)
			}
			metrics := n.observationMetrics()
			if metrics.Scope != "invocation" || metrics.Total.ReadCalls != uint64(src.reads) || metrics.Total.RequestedBytes == 0 || metrics.Total.ActualBytes == 0 {
				t.Fatal("physical reads missing", metrics)
			}
			var sum memory.Stats
			for _, stage := range metrics.Stages {
				sum = addStats(sum, stage.Stats)
			}
			if sum.ReadCalls != metrics.Total.ReadCalls || sum.Candidates != metrics.Total.Candidates {
				t.Fatal("stage counters duplicate/drop reads", sum, metrics.Total)
			}
			if n.Hints != nil {
				for _, hint := range n.Hints.Entries {
					if hint.Header.Kind == bridge.MemoryBody {
						t.Fatal("BODY leaked into general hints")
					}
				}
			}
		})
	}
}

func TestObservationRejectsChangedHeadMappingIdentityAndCancellation(t *testing.T) {
	for _, mode := range []string{"changed_head", "mapping_changed", "short_head", "process_changed", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			n, src, q, head, body := observationFixture(t, 544<<10, true)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			bodyRead := false
			src.afterRead = func(addr uint64, out []byte) {
				if addr != body || len(out) <= bridge.MemoryHeaderBytes {
					return
				}
				bodyRead = true
				switch mode {
				case "changed_head":
					src.data[head+bridge.MemoryHeaderBytes] ^= 1
				case "process_changed":
					src.verifyError = errors.New("fixture process reused")
				case "cancelled":
					cancel()
				}
			}
			src.beforeRead = func(addr uint64, out []byte) (int, error, bool) {
				if !bodyRead || addr != head {
					return 0, nil, false
				}
				if mode == "mapping_changed" {
					return 0, errors.New("fixture current mapping rejected"), true
				}
				if mode == "short_head" {
					return copy(out[:len(out)-1], src.data[addr:]), nil, true
				}
				return 0, nil, false
			}
			got, err := ObserveRecords(ctx, observationReader{n, src}, q)
			if err == nil || len(got.Payload) != 0 || !bodyRead {
				t.Fatal("mixed report accepted", mode, got, err)
			}
			if mode == "cancelled" && !errors.Is(err, context.Canceled) {
				t.Fatal("cancellation hidden", err)
			}
			if mode == "short_head" && n.observationMetrics().Total.ShortReads == 0 {
				t.Fatal("short read missing")
			}
		})
	}
}

func TestObservationSessionRetainsBudgetAndDeadlineOnRuntimeChange(t *testing.T) {
	n := &Native{Target: desktop.WindowIdentity{ProcessID: 7, ProcessStartedAt: 132000000000000000, Executable: "fixture.exe"}}
	n.memorySession()
	n.observation.session = memory.NewSession(memory.Budget{MaxReadCalls: 1})
	deadline := time.Now().Add(time.Minute)
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	if err := n.beginObservation(ctx, Identity{Runtime: strings.Repeat("1", 32)}); err != nil {
		t.Fatal(err)
	}
	src := &observationSource{data: make([]byte, 16)}
	if _, err := n.memorySource(src).Read(ctx, 0, make([]byte, 1)); err != nil {
		t.Fatal(err)
	}
	later, stop := context.WithDeadline(context.Background(), deadline.Add(time.Minute))
	defer stop()
	if err := n.beginObservation(later, Identity{Runtime: strings.Repeat("8", 32)}); err != nil {
		t.Fatal(err)
	}
	if n.observation.deadline != deadline || n.observation.session.Stats().ReadCalls != 1 {
		t.Fatal("runtime renewed deadline/credit")
	}
	if _, err := n.memorySource(src).Read(later, 0, make([]byte, 1)); !errors.Is(err, memory.ErrBudget) || src.reads != 1 {
		t.Fatal("global physical budget reset", err, src.reads)
	}
	n.Target.ProcessStartedAt++
	if err := n.beginObservation(ctx, Identity{}); err == nil {
		t.Fatal("changed target admitted")
	}
}

func TestObservationHeadAndBodySharePhysicalBudget(t *testing.T) {
	n, src, q, _, _ := observationFixture(t, 544<<10, true)
	n.memorySession()
	n.observation.session = memory.NewSession(memory.Budget{MaxReadCalls: 2})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	got, err := ObserveRecords(ctx, observationReader{n, src}, q)
	if !errors.Is(err, memory.ErrBudget) || len(got.Payload) != 0 || src.reads != 2 {
		t.Fatal("BODY renewed HEAD budget", got, err, src.reads)
	}
	if !n.observationMetrics().Total.BudgetExhausted {
		t.Fatal("exhaustion absent from result")
	}
}

func TestObservationInvalidHeadCannotAuthorizeBodyPointReads(t *testing.T) {
	n, src, q, head, body := observationFixture(t, 544<<10, true)
	header, err := bridge.DecodeMemoryHeader(src.data[head:])
	if err != nil {
		t.Fatal(err)
	}
	header, payload, err := bridge.DecodeMemoryRecord(src.data[head : head+bridge.MemoryHeaderBytes+uint64(header.Length)+bridge.MemoryTrailerBytes])
	if err != nil {
		t.Fatal(err)
	}
	var receipt Receipt
	if err := json.Unmarshal(payload, &receipt); err != nil {
		t.Fatal(err)
	}
	receipt.Owner = strings.Repeat("8", 32)
	payload, _ = json.Marshal(receipt)
	raw, err := bridge.EncodeMemoryRecord(header, payload)
	if err != nil {
		t.Fatal(err)
	}
	copy(src.data[head:], raw)
	bodyPointReads := 0
	src.beforeRead = func(addr uint64, out []byte) (int, error, bool) {
		if addr == body {
			bodyPointReads++
		}
		return 0, nil, false
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	got, err := ObserveRecords(ctx, observationReader{n, src}, q)
	if !errors.Is(err, ErrPending) || len(got.Payload) != 0 || bodyPointReads != 0 {
		t.Fatal("foreign HEAD authorized BODY", got, err, bodyPointReads)
	}
}

func TestObservationNestedDeadlineDoesNotExpireOuterSession(t *testing.T) {
	n := &Native{}
	outer, cancel := context.WithTimeout(context.Background(), time.Minute)
	defer cancel()
	if err := n.beginObservation(outer, Identity{}); err != nil {
		t.Fatal(err)
	}
	deadline := n.observation.deadline
	inner, stop := context.WithTimeout(outer, time.Millisecond)
	defer stop()
	bounded, end := n.observationContext(inner)
	defer end()
	if d, _ := bounded.Deadline(); !d.Before(deadline) {
		t.Fatal("nested context expanded")
	}
	if n.observation.deadline != deadline {
		t.Fatal("nested discovery consumed outer deadline")
	}
	data, err := json.Marshal(n.observationMetrics())
	if err != nil || !strings.Contains(string(data), `"processCreated":"0"`) {
		t.Fatal("identity lost string encoding", string(data), err)
	}
}

func TestObservationReplacementSnapshotBudgetIsExplicit(t *testing.T) {
	old, fixture := replacementFixture()
	record, raw := relocationRecord(t, 128, fixture.Witness.Observation, 1)
	src := &observationSource{data: make([]byte, 8192)}
	copy(src.data[128:], raw)
	session := memory.NewSession(memory.Budget{MaxRequestedBytes: 16})
	proof, err := observeReplacementBytes(context.Background(), old, []Identity{fixture.Current}, 1, 2, session.Source(src), []memory.Record{record}, func(context.Context) error { t.Fatal("budget failure waited for another snapshot"); return nil }, func(context.Context, memory.Selector, bool) (memory.LookupResult, error) {
		t.Fatal("budget failure started discovery")
		return memory.LookupResult{}, nil
	})
	if proof != nil || !errors.Is(err, memory.ErrBudget) || src.reads != 0 {
		t.Fatal("snapshot exhausted silently", proof, err, src.reads)
	}
}

func TestObservationRepeatedStagesPreserveAllStatistics(t *testing.T) {
	n, src, q, _, _ := observationFixture(t, 544<<10, true)
	nonce, _ := tokenBytes(q.Envelope.Nonce)
	runtime, _ := tokenBytes(q.Envelope.Runtime)
	selector := memory.Selector{Kind: bridge.MemoryReceipt, Nonce: nonce, Runtime: runtime, Accept: func(memory.Record) bool { return false }}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	for i := 0; i < 2; i++ {
		if _, err := n.find(ctx, src, selector, false); err != nil {
			t.Fatal(err)
		}
	}
	metrics := n.observationMetrics()
	if metrics.Total.LearningBytes == 0 || metrics.Total.PredicateRejected == 0 {
		t.Fatal("fixture failed to exercise learning/rejection", metrics.Total)
	}
	var sum memory.Stats
	for _, stage := range metrics.Stages {
		sum = sum.Add(stage.Stats)
	}
	if sum != metrics.Total || metrics.Stages[0].Calls != 2 {
		t.Fatal("repeated stage dropped fields", sum, metrics.Total, metrics.Stages)
	}
}

func TestObservationStageOverflowRemainsVisibleAndCounted(t *testing.T) {
	n := &Native{}
	src := &observationSource{data: make([]byte, 64)}
	for i := 0; i < 40; i++ {
		before := n.memorySession().Stats()
		if _, err := n.memorySource(src).Read(context.Background(), 0, make([]byte, 1)); err != nil {
			t.Fatal(err)
		}
		n.recordLookup(memory.Selector{Kind: bridge.MemoryIdentity}, memory.LookupResult{Path: fmt.Sprint("fixture-", i), Stats: n.memorySession().Stats().Delta(before)})
	}
	metrics := n.observationMetrics()
	if !metrics.StageTruncated || len(metrics.Stages) != observationStageLimit || metrics.Stages[len(metrics.Stages)-1].Path != "unattributed" {
		t.Fatal("stage cap silently dropped attribution", metrics)
	}
	var sum memory.Stats
	for _, stage := range metrics.Stages {
		sum = sum.Add(stage.Stats)
	}
	if sum != metrics.Total || sum.ReadCalls != 40 {
		t.Fatal("stage overflow dropped physical work", sum, metrics.Total)
	}
}
