package memory

import (
	"context"
	"errors"
	"hash/adler32"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func optimizationFixture(tb testing.TB, noise int) (*fakeSource, Selector) {
	tb.Helper()
	s := source(4 << 20)
	for i := 0; i < noise; i++ {
		h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Runtime: [16]byte{1}, Nonce: [16]byte{1}, Sequence: uint32(i + 1)}
		b, err := bridge.EncodeMemoryRecord(h, []byte("old input"))
		if err != nil {
			tb.Fatal(err)
		}
		copy(s.data[128+i*256:], b)
	}
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1, Runtime: [16]byte{2}, Nonce: [16]byte{7, 8, 9, 10, 11}, Ticket: [16]byte{3}}
	b, err := bridge.EncodeMemoryRecord(h, []byte("wanted"))
	if err != nil {
		tb.Fatal(err)
	}
	copy(s.data[3<<20:], b)
	return s, Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce, Ticket: h.Ticket}
}

func TestExactNonceWithHintsAndIndependentLearningBudget(t *testing.T) {
	s, selector := optimizationFixture(t, 10000)
	for _, enabled := range []bool{false, true} {
		var hints *Hints
		if enabled {
			hints = LoadHints("missing", "scope")
		}
		session := NewSession(Budget{MaxLearningCandidates: 2})
		records, coverage, err := lookup(context.Background(), s, selector, Options{Workers: 1, Session: session}, false, hints)
		if err != nil || len(records) != 1 || !coverage.Complete || coverage.Workers[0].Candidates != 2 {
			t.Fatalf("enabled=%t records=%d coverage=%+v err=%v", enabled, len(records), coverage, err)
		}
		st := session.Stats()
		if st.ReadCalls != 6 || st.ActualBytes != st.RequestedBytes || st.Candidates != 3 {
			t.Fatalf("physical/header/trailer accounting: %+v", st)
		}
		if enabled && (st.LearningCandidates != 2 || !st.LearningExhausted) {
			t.Fatalf("learning budget: %+v", st)
		}
	}
}

func TestSessionBudgetPersistsAcrossFindAndPointReads(t *testing.T) {
	s := source(8192)
	selector := hintFixture(t, s, 128, "wanted")
	r, err := ReadRecord(context.Background(), s, 128, selector)
	if err != nil {
		t.Fatal(err)
	}
	hints := LoadHints("missing", "scope")
	hints.learn(r)
	s.calls = 0
	session := NewSession(Budget{MaxReadCalls: 2})
	got, err := FindWithSession(context.Background(), session.Source(s), selector, hints, true, session)
	if err != nil || got.Path != "cache_hit" || got.Stats.ReadCalls != 2 {
		t.Fatalf("first=%+v err=%v", got, err)
	}
	got, err = FindWithSession(context.Background(), s, selector, hints, true, session)
	if !errors.Is(err, ErrBudget) || got.Coverage.Complete || s.calls != 2 || session.Stats().ReadCalls != 2 {
		t.Fatalf("renewed budget got=%+v stats=%+v calls=%d err=%v", got, session.Stats(), s.calls, err)
	}
	if _, err = ReadRecord(context.Background(), session.Source(s), 128, selector); !errors.Is(err, ErrBudget) {
		t.Fatalf("point bypass=%v", err)
	}
}

func TestExplicitSessionOwnershipCannotBypassTightBudget(t *testing.T) {
	s := source(8192)
	selector := hintFixture(t, s, 128, "wanted")
	s.calls = 0
	loose := NewSession(Budget{})
	tight := NewSession(Budget{MaxReadCalls: 1})
	wrapped := loose.Source(s)
	if _, err := ReadRecord(context.Background(), tight.Source(wrapped), 128, selector); !errors.Is(err, ErrSessionOwnership) {
		t.Fatalf("nested ownership=%v", err)
	}
	if _, err := FindWithSession(context.Background(), wrapped, selector, nil, true, tight); !errors.Is(err, ErrSessionOwnership) {
		t.Fatalf("explicit ownership=%v", err)
	}
	if s.calls != 0 || loose.Stats().ReadCalls != 0 || tight.Stats().ReadCalls != 0 {
		t.Fatal("conflict performed physical read")
	}
	if _, err := ReadRecord(context.Background(), wrapped, 128, selector); err != nil || loose.Stats().ReadCalls != 2 {
		t.Fatalf("implicit owner reuse=%v stats=%+v", err, loose.Stats())
	}
	if loose.Source(wrapped) != wrapped {
		t.Fatal("same owner wrapper not idempotent")
	}
}

func TestHintLearningClampsCallerConstructedOverflow(t *testing.T) {
	hints := LoadHints("missing", "scope")
	hints.Entries = make([]Hint, 10000)
	for i := range hints.Entries {
		hints.Entries[i] = Hint{Address: uint64(i + 1), Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: [16]byte{1}}}
	}
	hints.learn(Record{Address: 20000, Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: [16]byte{1}}})
	if len(hints.Entries) != 64 || cap(hints.Entries) > 128 || hints.Entries[0].Address != 20000 {
		t.Fatalf("oversized collection retained len=%d cap=%d", len(hints.Entries), cap(hints.Entries))
	}
}

func TestSessionCountsOverlapSalvageAndShortReads(t *testing.T) {
	s := source(16384)
	s.transient = true
	session := NewSession(Budget{})
	r, err := Scan(context.Background(), s, [][]byte{[]byte("12345678")}, Options{Workers: 1, ChunkBytes: 8192, Session: session})
	if err != nil || !r.Coverage.Complete {
		t.Fatalf("%+v %v", r.Coverage, err)
	}
	st := session.Stats()
	if st.ReadCalls != 7 || st.ShortReads != 2 || st.ActualBytes != 16391 || st.RequestedBytes != 32782 || st.RegionCalls != 2 {
		t.Fatalf("unaccounted salvage/overlap: %+v", st)
	}
	limited := NewSession(Budget{MaxRequestedBytes: 8192})
	r, err = Scan(context.Background(), s, [][]byte{[]byte("12345678")}, Options{Workers: 1, ChunkBytes: 8192, Session: limited})
	if !errors.Is(err, ErrBudget) || r.Coverage.Complete || !r.Coverage.Truncated || limited.Stats().RequestedBytes > 8192 {
		t.Fatalf("limited=%+v stats=%+v err=%v", r.Coverage, limited.Stats(), err)
	}
}

func TestSessionConcurrentReservationAndCancellation(t *testing.T) {
	s := source(8192)
	session := NewSession(Budget{MaxReadCalls: 7, MaxRequestedBytes: 70})
	src := session.Source(s)
	var wg sync.WaitGroup
	for i := 0; i < 40; i++ {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = src.Read(context.Background(), 0, make([]byte, 10)) }()
	}
	wg.Wait()
	if st := session.Stats(); st.ReadCalls != 7 || st.RequestedBytes != 70 || st.ActualBytes != 70 || !st.BudgetExhausted {
		t.Fatalf("reservation race: %+v", st)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := src.Read(ctx, 0, make([]byte, 10)); !errors.Is(err, context.Canceled) || !session.Stats().Cancelled {
		t.Fatalf("cancel=%v %+v", err, session.Stats())
	}
}

type localTimeoutThenReadable struct {
	*fakeSource
	delayed atomic.Bool
}

func (s *localTimeoutThenReadable) Read(ctx context.Context, at uint64, b []byte) (int, error) {
	if s.delayed.CompareAndSwap(false, true) {
		<-ctx.Done()
		return 0, ctx.Err()
	}
	return s.fakeSource.Read(ctx, at, b)
}
func TestLocalTimeoutDoesNotCancelSuccessfulFullFallback(t *testing.T) {
	s, _, hints := nearbyFixture(t, 6<<20, 1<<20)
	wanted := hintFixture(t, s.fakeSource, 5<<20, "new-nonce")
	slow := &localTimeoutThenReadable{fakeSource: s.fakeSource}
	session := NewSession(Budget{})
	got, err := FindWithSession(context.Background(), slow, wanted, hints, true, session)
	st := session.Stats()
	if err != nil || len(got.Records) != 1 || !st.LocalStopped || st.Cancelled || st.DeadlineExceeded || st.BudgetExhausted {
		t.Fatalf("got=%+v stats=%+v err=%v", got, st, err)
	}
}

func TestLearningByteBudgetDoesNotStopBusinessCoverage(t *testing.T) {
	s, selector := optimizationFixture(t, 10000)
	session := NewSession(Budget{MaxLearningBytes: 1})
	records, coverage, err := lookup(context.Background(), s, selector, Options{Session: session, Workers: 1}, false, LoadHints("missing", "scope"))
	if err != nil || len(records) != 1 || !coverage.Complete || !session.Stats().LearningExhausted || session.Stats().LearningBytes != 0 {
		t.Fatalf("records=%d coverage=%+v stats=%+v err=%v", len(records), coverage, session.Stats(), err)
	}
	limited := NewSession(Budget{MaxCandidates: 1})
	records, coverage, err = Lookup(context.Background(), s, selector, Options{Session: limited, Workers: 1}, false)
	if !errors.Is(err, ErrBudget) || len(records) != 0 || coverage.Complete {
		t.Fatalf("candidate budget records=%d coverage=%+v err=%v", len(records), coverage, err)
	}
}

func TestNewNonceNearbyAndDistantFallback(t *testing.T) {
	for _, next := range []int{(1 << 20) + 4096, 5 << 20} {
		s, old, hints := nearbyFixture(t, 6<<20, 1<<20)
		wanted := hintFixture(t, s.fakeSource, next, "new-nonce")
		wanted.Ticket = old.Ticket
		got, err := Find(context.Background(), s, wanted, hints, true)
		if err != nil || len(got.Records) != 1 || got.Records[0].Address != uint64(next) {
			t.Fatalf("next=%d got=%+v err=%v", next, got, err)
		}
		if next < 2<<20 && (got.Path != "nearby_scan" || s.regionsCalls.Load() != 0 || got.Coverage.Complete) {
			t.Fatalf("new nonce did not reuse scheduling: %+v", got)
		}
		if next > 2<<20 && s.regionsCalls.Load() == 0 {
			t.Fatal("local miss omitted full fallback")
		}
	}
}

func TestAuthorizedBodySeedsNeverPromoteHintsOrBypassAuthorization(t *testing.T) {
	s := source(2 << 20)
	head := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: [16]byte{2}, Nonce: [16]byte{3}, Ticket: [16]byte{4}}
	payload := []byte("verified result")
	body := head
	body.Kind = bridge.MemoryBody
	body.State = 3
	b, err := bridge.EncodeMemoryRecord(body, payload)
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[(1<<20)+4096:], b)
	selector := Selector{Kind: bridge.MemoryBody, Runtime: head.Runtime, Nonce: head.Nonce, Ticket: head.Ticket, BodyAuthorized: true, BodyLength: uint32(len(payload)), BodyChecksum: adler32.Checksum(payload)}
	seeds := []Hint{{Address: 1 << 20, Header: head}}
	got, err := FindNearbySeeded(context.Background(), s, selector, seeds, NewSession(Budget{}))
	if err != nil || len(got.Records) != 1 || string(got.Records[0].Payload) != string(payload) || got.Coverage.Complete {
		t.Fatalf("body=%+v err=%v", got, err)
	}
	selector.BodyChecksum ^= 1
	got, err = FindNearbySeeded(context.Background(), s, selector, seeds, nil)
	if err != nil || len(got.Records) != 0 {
		t.Fatalf("checksum bypass: %+v %v", got, err)
	}
	selector.BodyAuthorized = false
	s.calls = 0
	got, err = FindNearbySeeded(context.Background(), s, selector, seeds, nil)
	if err != nil || len(got.Records) != 0 || s.calls != 0 {
		t.Fatalf("HEAD authorization bypass: %+v %v", got, err)
	}
	hints := LoadHints("missing", "scope")
	hints.learn(Record{Address: 1, Header: body})
	if len(hints.Entries) != 0 {
		t.Fatal("BODY retained in general hints")
	}
}

func BenchmarkExactNonceWithHints(b *testing.B) {
	for _, enabled := range []bool{false, true} {
		name := "cache_off"
		if enabled {
			name = "hints"
		}
		b.Run(name, func(b *testing.B) {
			s, selector := optimizationFixture(b, 10000)
			b.ReportAllocs()
			b.SetBytes(int64(len(s.data)))
			b.ResetTimer()
			var candidates, learned uint64
			for i := 0; i < b.N; i++ {
				var hints *Hints
				if enabled {
					hints = LoadHints("missing", "scope")
				}
				r, c, err := lookup(context.Background(), s, selector, Options{Workers: 1}, false, hints)
				if err != nil || len(r) != 1 {
					b.Fatal(err)
				}
				candidates += c.Workers[0].Candidates
				learned += c.Stats.LearningCandidates
			}
			b.ReportMetric(float64(candidates)/float64(b.N), "anchors/op")
			b.ReportMetric(float64(learned)/float64(b.N), "learning/op")
		})
	}
}

func BenchmarkHintLearning(b *testing.B) {
	hints := LoadHints("missing", "scope")
	for i := 0; i < 64; i++ {
		hints.learn(Record{Address: uint64(i + 1), Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: [16]byte{1}}})
	}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		hints.learn(Record{Address: uint64(i%64 + 1), Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: [16]byte{1}}})
	}
}
