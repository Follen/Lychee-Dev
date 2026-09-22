package container_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"io"
	"math"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

type readerAtCall struct {
	offset int64
	length int
}

type synchronizedReaderAt struct {
	mu        sync.Mutex
	data      []byte
	shortFrom int64
	calls     []readerAtCall
}

func newSynchronizedReaderAt(data []byte) *synchronizedReaderAt {
	return &synchronizedReaderAt{data: data, shortFrom: -1}
}

func (r *synchronizedReaderAt) ReadAt(p []byte, offset int64) (int, error) {
	r.mu.Lock()
	r.calls = append(r.calls, readerAtCall{offset: offset, length: len(p)})
	data := r.data
	short := r.shortFrom >= 0 && offset >= r.shortFrom
	r.mu.Unlock()

	if offset < 0 {
		return 0, errors.New("negative ReaderAt offset")
	}
	if offset >= int64(len(data)) {
		return 0, io.EOF
	}
	n := copy(p, data[offset:])
	if n != len(p) {
		return n, io.EOF
	}
	if short {
		if n == 0 {
			return 0, io.ErrUnexpectedEOF
		}
		return n - 1, io.ErrUnexpectedEOF
	}
	return n, nil
}

func (r *synchronizedReaderAt) resetCalls() {
	r.mu.Lock()
	r.calls = nil
	r.mu.Unlock()
}

func (r *synchronizedReaderAt) callsSnapshot() []readerAtCall {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]readerAtCall(nil), r.calls...)
}

func openTestRanges(t *testing.T, input []byte, limits container.Limits) (*container.Ranges, *synchronizedReaderAt) {
	t.Helper()
	source := newSynchronizedReaderAt(input)
	ranges, err := container.OpenRanges(context.Background(), source, int64(len(input)), limits, nil)
	if err != nil {
		t.Fatalf("OpenRanges: %v", err)
	}
	return ranges, source
}

func assertCalls(t *testing.T, source *synchronizedReaderAt, want ...readerAtCall) {
	t.Helper()
	got := source.callsSnapshot()
	if len(got) != len(want) {
		t.Fatalf("ReaderAt calls = %#v, want %#v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("ReaderAt call %d = %#v, want %#v", i, got[i], want[i])
		}
	}
}

func blockPayloadStart(input []byte) int64 {
	return int64(binary.BigEndian.Uint32(input[4:8]))
}

func blockSpans(input []byte, blocks []fixtureBlock) []readerAtCall {
	spans := make([]readerAtCall, len(blocks))
	offset := blockPayloadStart(input)
	for i, block := range blocks {
		spans[i] = readerAtCall{offset: offset, length: len(block.data)}
		offset += int64(len(block.data))
	}
	return spans
}

func TestOpenRangesRequiresFramingAndReadsOnlyTheHeader(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("first"), plainBlock("second")}
	input := framedBLTE(blocks...)
	ranges, source := openTestRanges(t, input, generousLimits)

	if got, want := ranges.Size(), int64(len("firstsecond")); got != want {
		t.Fatalf("Size() = %d, want %d", got, want)
	}
	headerSize := blockPayloadStart(input)
	assertCalls(t, source,
		readerAtCall{offset: 0, length: 8},
		readerAtCall{offset: 8, length: int(headerSize) - 8},
	)

	source.resetCalls()
	got, err := ranges.ReadSpan(context.Background(), ranges.Size(), 0)
	if err != nil {
		t.Fatalf("zero-length EOF ReadSpan: %v", err)
	}
	if got == nil || len(got) != 0 {
		t.Fatalf("zero-length EOF result = %#v, want a non-nil empty result", got)
	}
	assertCalls(t, source)

	_, err = container.OpenRanges(context.Background(), newSynchronizedReaderAt(zeroHeaderBLTE(plainBlock("headerless").data)), int64(len(zeroHeaderBLTE(plainBlock("headerless").data))), generousLimits, nil)
	if !errors.Is(err, container.ErrUnsupported) {
		t.Fatalf("headerless OpenRanges error = %v, want ErrUnsupported", err)
	}
}

func TestOpenRangesChecksDeclaredPayloadSize(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("one"), plainBlock("two")}
	input := framedBLTE(blocks...)
	entry := 12
	declared := binary.BigEndian.Uint32(input[entry : entry+4])
	binary.BigEndian.PutUint32(input[entry:entry+4], declared+1)

	_, err := container.OpenRanges(context.Background(), newSynchronizedReaderAt(input), int64(len(input)), generousLimits, nil)
	if !errors.Is(err, container.ErrMalformed) {
		t.Fatalf("declared-size error = %v, want ErrMalformed", err)
	}
}

func TestOpenRangesRejectsResourceBudgets(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("first"), plainBlock("second")}
	input := framedBLTE(blocks...)
	headerSize := blockPayloadStart(input)
	decodedSize := int64(len("firstsecond"))
	cases := []struct {
		name  string
		limit func(*container.Limits)
	}{
		{name: "encoded bytes", limit: func(l *container.Limits) { l.EncodedBytes = int64(len(input)) - 1 }},
		{name: "decoded bytes", limit: func(l *container.Limits) { l.DecodedBytes = decodedSize - 1 }},
		{name: "chunk count", limit: func(l *container.Limits) { l.Chunks = len(blocks) - 1 }},
	}

	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			limits := generousLimits
			testCase.limit(&limits)
			_, err := container.OpenRanges(context.Background(), newSynchronizedReaderAt(input), int64(len(input)), limits, nil)
			if !errors.Is(err, container.ErrLimit) {
				t.Fatalf("OpenRanges error = %v, want ErrLimit", err)
			}
		})
	}
	if headerSize <= 0 {
		t.Fatal("test fixture has no framed header")
	}
}

func TestReadSpanReadsOnlyIntersectingBlocksAndDecodesThem(t *testing.T) {
	inner := zeroHeaderBLTE(plainBlock("fgh").data)
	blocks := []fixtureBlock{
		plainBlock("ab"),
		zlibBlock(t, "cde"),
		modeBlock('F', inner, len("fgh")),
		plainBlock("ij"),
	}
	input := framedBLTE(blocks...)
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()

	got, err := ranges.ReadSpan(context.Background(), 1, 7)
	if err != nil {
		t.Fatalf("ReadSpan: %v", err)
	}
	if string(got) != "bcdefgh" {
		t.Fatalf("ReadSpan = %q, want %q", got, "bcdefgh")
	}
	wantCalls := blockSpans(input, blocks)
	assertCalls(t, source, wantCalls[0], wantCalls[1], wantCalls[2])

	source.resetCalls()
	got, err = ranges.ReadSpan(context.Background(), 1, 7)
	if err != nil || string(got) != "bcdefgh" {
		t.Fatalf("repeated ReadSpan = %q, %v; want another successful source read", got, err)
	}
	assertCalls(t, source, wantCalls[0], wantCalls[1], wantCalls[2])
}

func TestReadSpanDoesNotFetchUntouchedCorruptBlocks(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("good"), plainBlock("untouched corrupt")}
	input := framedBLTE(blocks...)
	secondOffset := int(blockPayloadStart(input)) + len(blocks[0].data)
	input[secondOffset+1] ^= 0xff
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()

	got, err := ranges.ReadSpan(context.Background(), 0, int64(len("good")))
	if err != nil || string(got) != "good" {
		t.Fatalf("ReadSpan = %q, %v; want untouched corrupt block to be ignored", got, err)
	}
	assertCalls(t, source, readerAtCall{offset: blockPayloadStart(input), length: len(blocks[0].data)})
}

func TestReadSpanReturnsNoPartialResultForTouchedBadChecksum(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("good"), plainBlock("bad")}
	input := framedBLTE(blocks...)
	secondOffset := int(blockPayloadStart(input)) + len(blocks[0].data)
	input[secondOffset+1] ^= 0xff
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()

	got, err := ranges.ReadSpan(context.Background(), 0, int64(len("goodbad")))
	if !errors.Is(err, container.ErrIntegrity) {
		t.Fatalf("ReadSpan error = %v, want ErrIntegrity", err)
	}
	if got != nil {
		t.Fatalf("ReadSpan result = %q, want nil on touched checksum failure", got)
	}
	spans := blockSpans(input, blocks)
	assertCalls(t, source, spans[0], spans[1])
}

func TestReadSpanRejectsShortReaderAtReads(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("short read")}
	input := framedBLTE(blocks...)
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()
	source.shortFrom = blockPayloadStart(input)

	got, err := ranges.ReadSpan(context.Background(), 0, int64(len("short read")))
	if !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("ReadSpan error = %v, want io.ErrUnexpectedEOF", err)
	}
	if got != nil {
		t.Fatalf("short-read result = %q, want nil", got)
	}
}

func TestReadSpanRejectsInvalidRangesAndOutputBudget(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("1234"), plainBlock("5678")}
	input := framedBLTE(blocks...)
	ranges, _ := openTestRanges(t, input, generousLimits)
	size := ranges.Size()
	cases := []struct {
		name   string
		offset int64
		length int64
	}{
		{name: "negative offset", offset: -1, length: 1},
		{name: "negative length", offset: 0, length: -1},
		{name: "offset past end", offset: size + 1, length: 0},
		{name: "range past end", offset: size, length: 1},
		{name: "offset overflow", offset: math.MaxInt64, length: 1},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			got, err := ranges.ReadSpan(context.Background(), testCase.offset, testCase.length)
			if err == nil {
				t.Fatal("ReadSpan succeeded for invalid range")
			}
			if got != nil {
				t.Fatalf("invalid-range result = %q, want nil", got)
			}
		})
	}

	limits := generousLimits
	limits.ChunkBytes = 5
	limited, err := container.OpenRanges(context.Background(), newSynchronizedReaderAt(input), int64(len(input)), limits, nil)
	if err != nil {
		t.Fatalf("OpenRanges with per-block limit: %v", err)
	}
	got, err := limited.ReadSpan(context.Background(), 0, 6)
	if !errors.Is(err, container.ErrLimit) {
		t.Fatalf("oversized ReadSpan error = %v, want ErrLimit", err)
	}
	if got != nil {
		t.Fatalf("oversized ReadSpan result = %q, want nil", got)
	}
}

func TestReadSpanHonorsCancellation(t *testing.T) {
	input := framedBLTE(plainBlock("cancelled"))
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	got, err := ranges.ReadSpan(ctx, 0, int64(len("cancelled")))
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("ReadSpan error = %v, want context.Canceled", err)
	}
	if got != nil {
		t.Fatalf("cancelled result = %q, want nil", got)
	}
	assertCalls(t, source)
}

func TestReadSpanSupportsConcurrentReads(t *testing.T) {
	blocks := []fixtureBlock{plainBlock("0123"), plainBlock("4567"), plainBlock("89ab"), plainBlock("cdef")}
	input := framedBLTE(blocks...)
	ranges, source := openTestRanges(t, input, generousLimits)
	source.resetCalls()

	type request struct {
		offset int64
		want   string
	}
	requests := []request{
		{offset: 0, want: "012345"},
		{offset: 2, want: "23456789"},
		{offset: 6, want: "6789abcdef"},
		{offset: 10, want: "abcdef"},
	}
	errs := make(chan error, len(requests))
	var group sync.WaitGroup
	for _, request := range requests {
		request := request
		group.Add(1)
		go func() {
			defer group.Done()
			got, err := ranges.ReadSpan(context.Background(), request.offset, int64(len(request.want)))
			if err != nil {
				errs <- err
				return
			}
			if string(got) != request.want {
				errs <- errors.New("concurrent ReadSpan returned the wrong bytes")
			}
		}()
	}
	group.Wait()
	close(errs)
	for err := range errs {
		t.Fatal(err)
	}
	if len(source.callsSnapshot()) == 0 {
		t.Fatal("concurrent reads did not use the ReaderAt")
	}
}

func TestReadSpanUsesOriginalEncryptedBlockIndex(t *testing.T) {
	raw, key := cipherFixture(t, 16, 4)
	blocks := []fixtureBlock{
		plainBlock("prefix"),
		{data: raw, decoded: 130},
		plainBlock("suffix"),
	}
	input := framedBLTE(blocks...)
	lookup := func(ctx context.Context, name uint64) ([]byte, error) {
		if name != 0x1234567890abcdef {
			t.Fatalf("wrong key identity %x", name)
		}
		return key, nil
	}
	source := newSynchronizedReaderAt(input)
	ranges, err := container.OpenRanges(context.Background(), source, int64(len(input)), generousLimits, lookup)
	if err != nil {
		t.Fatalf("OpenRanges: %v", err)
	}
	source.resetCalls()

	got, err := ranges.ReadSpan(context.Background(), int64(len("prefix")), 130)
	if err != nil {
		t.Fatalf("encrypted ReadSpan: %v", err)
	}
	want := make([]byte, 130)
	for i := range want {
		want[i] = byte(i)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encrypted ReadSpan = %v, want bytes 0..129", got)
	}
	spans := blockSpans(input, blocks)
	assertCalls(t, source, spans[1])
}
