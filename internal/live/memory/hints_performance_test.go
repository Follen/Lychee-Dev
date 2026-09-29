package memory

import (
	"context"
	"github.com/follenfang/lycheedev/internal/bridge"
	"sync/atomic"
	"testing"
)

type countedHintsSource struct {
	*fakeSource
	regionsCalls atomic.Int32
	verifies     atomic.Int32
}

func (s *countedHintsSource) Regions(ctx context.Context) ([]Region, error) {
	s.regionsCalls.Add(1)
	return s.fakeSource.Regions(ctx)
}
func (s *countedHintsSource) Verify(context.Context) error { s.verifies.Add(1); return nil }
func hintFixture(t *testing.T, s *fakeSource, address int, nonce string) Selector {
	t.Helper()
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1}
	copy(h.Runtime[:], "known-runtime")
	copy(h.Nonce[:], nonce)
	copy(h.Ticket[:], "ticket")
	b, err := bridge.EncodeMemoryRecord(h, []byte("proof"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[address:], b)
	return Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce, Ticket: h.Ticket}
}
func TestKnownRuntimeHintAvoidsWholeRegionEnumeration(t *testing.T) {
	s := &countedHintsSource{fakeSource: source(8192)}
	selector := hintFixture(t, s.fakeSource, 128, "wanted")
	hints := LoadHints("missing", "scope")
	if _, err := Find(context.Background(), s, selector, hints, true); err != nil {
		t.Fatal(err)
	}
	s.regionsCalls.Store(0)
	s.verifies.Store(0)
	s.calls = 0
	got, err := Find(context.Background(), s, selector, hints, true)
	if err != nil || got.Path != "cache_hit" || s.regionsCalls.Load() != 0 || s.calls != 2 || s.verifies.Load() != 2 {
		t.Fatalf("path=%s err=%v regions=%d reads=%d verifies=%d", got.Path, err, s.regionsCalls.Load(), s.calls, s.verifies.Load())
	}
	s.regionsCalls.Store(0)
	got, err = Find(context.Background(), s, selector, hints, false)
	if err != nil || !got.Coverage.Complete || s.regionsCalls.Load() != 1 {
		t.Fatalf("audit=%+v regions=%d err=%v", got, s.regionsCalls.Load(), err)
	}
}
func TestScanLearnsRelatedSmallRecordWithoutExtraReads(t *testing.T) {
	s := &countedHintsSource{fakeSource: source(8192)}
	nearby := hintFixture(t, s.fakeSource, 128, "next-query")
	wanted := hintFixture(t, s.fakeSource, 1024, "wanted")
	hints := LoadHints("missing", "scope")
	if _, err := Find(context.Background(), s, wanted, hints, true); err != nil {
		t.Fatal(err)
	}
	if s.calls != 3 {
		t.Fatalf("cold reads=%d want chunk+header+record only", s.calls)
	}
	s.regionsCalls.Store(0)
	s.calls = 0
	got, err := Find(context.Background(), s, nearby, hints, true)
	if err != nil || got.Path != "cache_hit" || s.regionsCalls.Load() != 0 || s.calls != 2 {
		t.Fatalf("related path=%s err=%v regions=%d reads=%d", got.Path, err, s.regionsCalls.Load(), s.calls)
	}
}

func TestLearnedHintsRemainBoundedAndNeverAuthorizeBody(t *testing.T) {
	s := &countedHintsSource{fakeSource: source(1 << 16)}
	selector := hintFixture(t, s.fakeSource, 128, "first")
	for i := 1; i < 90; i++ {
		hintFixture(t, s.fakeSource, 128+i*256, "other")
	}
	body := bridge.MemoryHeader{Kind: bridge.MemoryBody, State: 3, Runtime: selector.Runtime, Nonce: selector.Nonce, Ticket: selector.Ticket}
	data, err := bridge.EncodeMemoryRecord(body, []byte("secret"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[30000:], data)
	hints := LoadHints("missing", "scope")
	got, err := Find(context.Background(), s, selector, hints, false)
	if err != nil || !got.Coverage.Complete || len(hints.Entries) > 64 {
		t.Fatalf("result=%+v hints=%d err=%v", got, len(hints.Entries), err)
	}
	for _, h := range hints.Entries {
		if h.Header.Kind == bridge.MemoryBody {
			t.Fatal("learned unsolicited BODY")
		}
	}
	selector.Kind = bridge.MemoryBody
	got, err = Find(context.Background(), s, selector, hints, true)
	if err != nil || len(got.Records) != 0 {
		t.Fatalf("unauthorized BODY accepted: %+v %v", got, err)
	}
}

func TestHintStillChecksCurrentBytesPredicateAndUnknownRuntime(t *testing.T) {
	s := &countedHintsSource{fakeSource: source(8192)}
	selector := hintFixture(t, s.fakeSource, 128, "wanted")
	hints := LoadHints("missing", "scope")
	if _, err := Find(context.Background(), s, selector, hints, true); err != nil {
		t.Fatal(err)
	}
	selector.Accept = func(Record) bool { return false }
	got, err := Find(context.Background(), s, selector, hints, true)
	if err != nil || len(got.Records) != 0 || got.Path == "cache_hit" {
		t.Fatalf("predicate bypassed: %+v %v", got, err)
	}
	selector.Accept = nil
	selector.Runtime = [16]byte{}
	s.regionsCalls.Store(0)
	got, err = Find(context.Background(), s, selector, hints, true)
	if err != nil || got.Path == "cache_hit" || s.regionsCalls.Load() != 1 {
		t.Fatalf("unknown runtime shortcut: %+v %v maps=%d", got, err, s.regionsCalls.Load())
	}
}

func TestChunkPrefilterAvoidsPointReadsForUnrelatedHeaders(t *testing.T) {
	s := &countedHintsSource{fakeSource: source(8192)}
	selector := hintFixture(t, s.fakeSource, 7000, "wanted")
	for i := 0; i < 20; i++ {
		hintFixture(t, s.fakeSource, 128+i*256, "other")
	}
	got, err := Find(context.Background(), s, selector, LoadHints("missing", "scope"), true)
	if err != nil || len(got.Records) != 1 || s.calls != 3 {
		t.Fatalf("reads=%d records=%d err=%v", s.calls, len(got.Records), err)
	}
}

func TestChunkHeaderBoundaryStillUsesFreshRecordRead(t *testing.T) {
	for _, at := range []int{61, 127, 191} {
		s := source(512)
		selector := hintFixture(t, s, at, "wanted")
		records, _, err := lookup(context.Background(), s, selector, Options{Workers: 1, ChunkBytes: 64}, true, LoadHints("missing", "scope"))
		if err != nil || len(records) != 1 || records[0].Address != uint64(at) {
			t.Fatalf("at=%d records=%v err=%v", at, records, err)
		}
	}
}

type stopDuringReadSource struct {
	*fakeSource
	stop *atomic.Bool
}

func (s *stopDuringReadSource) Read(context.Context, uint64, []byte) (int, error) {
	s.calls++
	s.stop.Store(true)
	return 0, nil
}
func TestEarlyStopAvoidsPageSalvageAndFurtherDispatch(t *testing.T) {
	var stopped atomic.Bool
	s := &stopDuringReadSource{fakeSource: source(8192), stop: &stopped}
	stats := Worker{}
	refreshes := 0
	readSpan(context.Background(), s, 0, make([]byte, 8192), &stats, func() []Range { refreshes++; return []Range{{0, 8192}} }, stopped.Load)
	if s.calls != 1 || refreshes != 0 {
		t.Fatalf("stopped salvage reads=%d maps=%d", s.calls, refreshes)
	}
	source := source(1 << 20)
	copy(source.data, []byte("target"))
	result, err := Scan(context.Background(), source, [][]byte{[]byte("target")}, Options{Workers: 1, ChunkBytes: 64, Visit: func(Hit) bool { return true }})
	if err != nil || source.calls != 1 || result.Coverage.Workers[0].PlannedBytes >= 1<<20 || !result.Coverage.Truncated {
		t.Fatalf("dispatch continued: calls=%d coverage=%+v err=%v", source.calls, result.Coverage, err)
	}
}

func TestScanDoesNotLearnCorruptOrOtherRuntimeRecords(t *testing.T) {
	s := source(8192)
	wanted := hintFixture(t, s, 2048, "wanted")
	hintFixture(t, s, 128, "corrupt")
	s.data[128+bridge.MemoryHeaderBytes] ^= 1
	other := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1, Runtime: [16]byte{9}}
	data, err := bridge.EncodeMemoryRecord(other, []byte("proof"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[512:], data)
	hints := LoadHints("missing", "scope")
	if _, err := Find(context.Background(), s, wanted, hints, true); err != nil {
		t.Fatal(err)
	}
	for _, hint := range hints.Entries {
		if hint.Address == 128 || hint.Address == 512 {
			t.Fatalf("unvalidated or unrelated hint learned: %+v", hint)
		}
	}
}

func telemetryFixture(t *testing.T, s *fakeSource, at int, runtime [16]byte, sequence uint32) Record {
	t.Helper()
	header := bridge.MemoryHeader{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime, State: 1, Sequence: sequence}
	data, err := bridge.EncodeMemoryRecord(header, []byte("sample"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[at:], data)
	record, err := ReadRecord(context.Background(), s, uint64(at), Selector{Kind: header.Kind, Runtime: runtime, Nonce: runtime})
	if err != nil {
		t.Fatal(err)
	}
	return record
}

func TestTelemetryHintsRetainNewRuntimeAcrossUnknownRuntimeScan(t *testing.T) {
	s := source(1 << 20)
	hints := LoadHints("missing", "scope")
	current := [16]byte{2}
	old := [16]byte{1}
	for i := 0; i < 12; i++ {
		hints.learn(telemetryFixture(t, s, 128+i*256, current, uint32(i+1)))
	}
	// The following complete identity lookup encounters only old-runtime input
	// records. Its incidental learning must not evict newer-runtime locations.
	for i := 0; i < 100; i++ {
		telemetryFixture(t, s, 16384+i*256, old, uint32(10000+i))
	}
	result, err := Find(context.Background(), s, Selector{Kind: bridge.MemoryIdentity}, hints, false)
	if err != nil || !result.Coverage.Complete {
		t.Fatalf("result=%+v err=%v", result, err)
	}
	var seq []uint32
	for _, hint := range hints.Entries {
		if hint.Header.Runtime == current {
			seq = append(seq, hint.Header.Sequence)
		}
	}
	if len(seq) != 8 || seq[0] != 12 || seq[7] != 5 || hints.Entries[0].Header.Runtime != current {
		t.Fatalf("current sequences=%v first=%+v", seq, hints.Entries[0])
	}
	if len(hints.Entries) > 64 {
		t.Fatal("unbounded cache")
	}
}

func TestLateTelemetryBeyondLearningPrefixSeedsNewAddressSearch(t *testing.T) {
	s := source(4 << 20)
	hints := LoadHints("missing", "scope")
	runtime := [16]byte{2}
	for i := 0; i < 80; i++ {
		telemetryFixture(t, s, 128+i*256, runtime, uint32(i+1))
	}
	late := 3 << 20
	telemetryFixture(t, s, late, runtime, 1000)
	selector := Selector{Kind: bridge.MemoryInputState, Runtime: runtime, Nonce: runtime, Accept: func(Record) bool { return false }}
	// None is fresh enough to authorize input, but the last sequence is still a
	// useful location hint. All accepted result semantics remain unchanged.
	records, coverage, err := lookup(context.Background(), s, selector, Options{Workers: 1}, false, hints)
	if err != nil || len(records) != 0 || !coverage.Complete {
		t.Fatalf("records=%d coverage=%+v err=%v", len(records), coverage, err)
	}
	if len(hints.Entries) != 8 || hints.Entries[0].Header.Sequence != 1000 {
		t.Fatalf("newest candidate hidden beyond old prefix: %+v", hints.Entries)
	}
	telemetryFixture(t, s, late+4096, runtime, 1001)
	selector.Accept = func(r Record) bool { return r.Header.Sequence == 1001 }
	found, err := FindNearby(context.Background(), s, selector, hints)
	if err != nil || len(found.Records) != 1 || found.Records[0].Address != uint64(late+4096) || found.Coverage.Complete {
		t.Fatalf("nearby=%+v err=%v", found, err)
	}
	// Cache-off full audit finds precisely the same accepted record.
	off, _, err := Lookup(context.Background(), s, selector, Options{Workers: 1}, false)
	if err != nil || len(off) != 1 || off[0].Address != found.Records[0].Address {
		t.Fatalf("cache changed acceptance: %v %v", off, err)
	}
}

func TestTelemetryHintsKeepRoomAmongCurrentRuntimeReceipts(t *testing.T) {
	hints := LoadHints("missing", "scope")
	runtime := [16]byte{2}
	for i := 0; i < 80; i++ {
		hints.learn(Record{Address: uint64(i + 1), Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt, Runtime: runtime}})
	}
	hints.learn(Record{Address: 1000, Header: bridge.MemoryHeader{Kind: bridge.MemoryInputState, Runtime: runtime, Sequence: 100}})
	if len(hints.Entries) != 64 || hints.Entries[0].Header.Kind != bridge.MemoryInputState {
		t.Fatal("receipt history crowded out input location")
	}
}
