package records

import (
	"bytes"
	"compress/zlib"
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"io"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
)

// These are synthetic Encoding objects, not captures of a Retail build. Each
// page is valid, independently checksummed and separately framed. Compression
// keeps the fixture small without materializing the complete logical object.
type encodingBudgetFixture struct {
	raw          []byte
	logicalBytes int64
	contentKey   string
	encodingKey  string
	blockStarts  []int64
}

func makeEncodingBudgetFixture(t *testing.T, contentPages, encodedPages, pageKiB int) encodingBudgetFixture {
	t.Helper()
	pageBytes := pageKiB * 1024
	header := make([]byte, 22)
	copy(header, "EN")
	header[2], header[3], header[4] = 1, 16, 16
	binary.BigEndian.PutUint16(header[5:7], uint16(pageKiB))
	binary.BigEndian.PutUint16(header[7:9], uint16(pageKiB))
	binary.BigEndian.PutUint32(header[9:13], uint32(contentPages))
	binary.BigEndian.PutUint32(header[13:17], uint32(encodedPages))
	contentDirectory := make([]byte, contentPages*32)
	encodedDirectory := make([]byte, encodedPages*32)
	var contentBlocks, encodedBlocks [][]byte
	key := func(i int) [16]byte {
		var value [16]byte
		binary.BigEndian.PutUint32(value[:4], uint32(i+1))
		return value
	}
	compress := func(body []byte) []byte {
		var output bytes.Buffer
		output.WriteByte('Z')
		writer := zlib.NewWriter(&output)
		if _, err := writer.Write(body); err != nil {
			t.Fatal(err)
		}
		if err := writer.Close(); err != nil {
			t.Fatal(err)
		}
		return output.Bytes()
	}
	for i := 0; i < contentPages; i++ {
		page := make([]byte, pageBytes)
		ckey, ekey := key(i), key(0)
		page[0] = 1
		putUint40Test(page[1:6], 40)
		copy(page[6:22], ckey[:])
		copy(page[22:38], ekey[:])
		copy(contentDirectory[i*32:], ckey[:])
		digest := md5.Sum(page)
		copy(contentDirectory[i*32+16:], digest[:])
		contentBlocks = append(contentBlocks, compress(page))
	}
	for i := 0; i < encodedPages; i++ {
		page := make([]byte, pageBytes)
		ekey := key(i)
		copy(page, ekey[:])
		putUint40Test(page[20:25], 49)
		copy(encodedDirectory[i*32:], ekey[:])
		digest := md5.Sum(page)
		copy(encodedDirectory[i*32+16:], digest[:])
		encodedBlocks = append(encodedBlocks, compress(page))
	}
	blocks := [][]byte{append([]byte{'N'}, header...), append([]byte{'N'}, contentDirectory...)}
	decoded := []int{len(header), len(contentDirectory)}
	blocks = append(blocks, contentBlocks...)
	for range contentBlocks {
		decoded = append(decoded, pageBytes)
	}
	blocks = append(blocks, append([]byte{'N'}, encodedDirectory...))
	decoded = append(decoded, len(encodedDirectory))
	blocks = append(blocks, encodedBlocks...)
	for range encodedBlocks {
		decoded = append(decoded, pageBytes)
	}
	headerBytes := 12 + 24*len(blocks)
	raw := make([]byte, headerBytes)
	copy(raw, "BLTE")
	binary.BigEndian.PutUint32(raw[4:8], uint32(headerBytes))
	raw[8], raw[9], raw[10], raw[11] = 0x0f, byte(len(blocks)>>16), byte(len(blocks)>>8), byte(len(blocks))
	fixture := encodingBudgetFixture{}
	for i, block := range blocks {
		entry := raw[12+i*24 : 12+(i+1)*24]
		binary.BigEndian.PutUint32(entry[:4], uint32(len(block)))
		binary.BigEndian.PutUint32(entry[4:8], uint32(decoded[i]))
		digest := md5.Sum(block)
		copy(entry[8:], digest[:])
		fixture.blockStarts = append(fixture.blockStarts, int64(len(raw)))
		raw = append(raw, block...)
		fixture.logicalBytes += int64(decoded[i])
	}
	first := key(0)
	fixture.raw, fixture.contentKey, fixture.encodingKey = raw, hex.EncodeToString(first[:]), hex.EncodeToString(first[:])
	return fixture
}

type encodingBudgetObject struct {
	*bytes.Reader
	mu      sync.Mutex
	reads   [][2]int64
	closed  int
	failAt  int64
	failure error
}

func (o *encodingBudgetObject) ReadAt(p []byte, offset int64) (int, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.reads = append(o.reads, [2]int64{offset, offset + int64(len(p))})
	if o.failure != nil && offset == o.failAt {
		return 0, o.failure
	}
	return o.Reader.ReadAt(p, offset)
}

func (o *encodingBudgetObject) Close() error { o.closed++; return nil }

func (o *encodingBudgetObject) touched(start, end int64) bool {
	for _, span := range o.reads {
		if span[0] < end && span[1] > start {
			return true
		}
	}
	return false
}

func encodingBudgetSource(f encodingBudgetFixture) (fileSource, *encodingBudgetObject, *int) {
	object := &encodingBudgetObject{Reader: bytes.NewReader(f.raw)}
	opens := new(int)
	digest := md5.Sum(f.raw[:binary.BigEndian.Uint32(f.raw[4:8])])
	return fileSource{meta: BuildMetadata{ContentBytes: f.logicalBytes, EncodingBytes: int64(len(f.raw)), EncodingKey: hex.EncodeToString(digest[:])}, open: func(context.Context, string, int64) (encodedObject, error) {
		*opens++
		return object, nil
	}}, object, opens
}

func TestFileSourceEncodingLargeLogicalObjectFitsDefaultBudget(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 192, 1, 1024)
	budget := resource.New(resource.DefaultLimits())
	if f.logicalBytes <= resource.DefaultLimits().MetadataBytes || len(f.raw) >= 1<<20 {
		t.Fatalf("fixture is not large logical/small physical: decoded=%d encoded=%d", f.logicalBytes, len(f.raw))
	}
	source, object, opens := encodingBudgetSource(f)
	index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget, MetadataBytes: 1 << 30})
	if err != nil {
		t.Fatalf("bounded Encoding lookup rejected logical extent: %v (opens=%d decoded=%d encoded=%d)", err, *opens, f.logicalBytes, len(f.raw))
	}
	defer closeIndex()
	for i := 0; i < 3; i++ {
		content, err := index.FindContent(ctx, f.contentKey)
		if err != nil || content.DecodedBytes != 40 || len(content.EncodingKeys) != 1 || content.EncodingKeys[0] != f.encodingKey {
			t.Fatalf("small content lookup: %+v %v", content, err)
		}
		physical, err := index.FindEncoding(ctx, f.encodingKey)
		if err != nil || physical.EncodedBytes != 49 {
			t.Fatalf("small physical lookup: %+v %v", physical, err)
		}
	}
	if *opens != 1 || object.touched(f.blockStarts[3], f.blockStarts[194]) {
		t.Fatal("small lookup opened repeatedly or fetched unrelated CKey pages")
	}
	usage := budget.Snapshot()
	if usage.MetadataBytes >= 8<<20 || usage.RetainedBytes >= 16<<20 || budget.PeakScratchBytes() == 0 {
		t.Fatalf("logical extent charged instead of bounded metadata/cache: usage=%+v scratch=%d", usage, budget.PeakScratchBytes())
	}
}

func TestEncodingCKeyDirectoryBudgetRejectsBeforeRead(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 128, 1, 1)
	limits := resource.DefaultLimits()
	budget := resource.New(limits)
	object := &encodingBudgetObject{Reader: bytes.NewReader(f.raw)}
	ranges, err := container.OpenRanges(ctx, object, int64(len(f.raw)), readLimits(int64(len(f.raw)), f.logicalBytes, budget), nil)
	if err != nil {
		t.Fatal(err)
	}
	// Leave enough for the header but not the verified CKey directory. The
	// directory occupies a separate block, so its payload reads are observable.
	if err := budget.Charge(resource.Cost{MetadataBytes: limits.MetadataBytes - budget.Snapshot().MetadataBytes - 64}); err != nil {
		t.Fatal(err)
	}
	index, err := openEncoding(ctx, ranges, budget)
	if !errors.Is(err, resource.ErrBudget) || index != nil || object.touched(f.blockStarts[1], f.blockStarts[2]) {
		t.Fatalf("CKey directory was read/materialized before budget rejection: index=%v err=%v reads=%v", index != nil, err, object.reads)
	}
}

func TestFileSourceEncodingEKeyDirectoryBudgetRejectsBeforeRead(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 1, 128, 1)
	limits := resource.DefaultLimits()
	budget := resource.New(limits)
	source, object, _ := encodingBudgetSource(f)
	index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget, MetadataBytes: 1 << 30})
	if err != nil {
		t.Fatal(err)
	}
	defer closeIndex()
	if err := budget.Charge(resource.Cost{MetadataBytes: limits.MetadataBytes - budget.Snapshot().MetadataBytes - 1}); err != nil {
		t.Fatal(err)
	}
	record, err := index.FindEncoding(ctx, f.encodingKey)
	if !errors.Is(err, resource.ErrBudget) || record != (EncodedRecord{}) || object.touched(f.blockStarts[3], f.blockStarts[4]) {
		t.Fatalf("EKey directory was read/materialized before budget rejection: record=%+v err=%v reads=%v", record, err, object.reads)
	}
}

func TestFileSourceEncodingPageBudgetRejectsBeforeRead(t *testing.T) {
	for _, dimension := range []string{"metadata", "retained"} {
		t.Run(dimension, func(t *testing.T) {
			f := makeEncodingBudgetFixture(t, 1, 1, 1)
			limits := resource.DefaultLimits()
			budget := resource.New(limits)
			source, object, _ := encodingBudgetSource(f)
			index, closeIndex, err := source.encodingIndex(context.Background(), FileQuery{budget: budget})
			if err != nil {
				t.Fatal(err)
			}
			defer closeIndex()
			cost := resource.Cost{}
			if dimension == "metadata" {
				cost.MetadataBytes = limits.MetadataBytes - budget.Snapshot().MetadataBytes - 1
			} else {
				cost.RetainedBytes = limits.RetainedBytes - budget.Snapshot().RetainedBytes - 1
			}
			if err := budget.Charge(cost); err != nil {
				t.Fatal(err)
			}
			before := budget.Snapshot()
			if record, err := index.FindContent(context.Background(), f.contentKey); !errors.Is(err, resource.ErrBudget) || record.ContentKey != "" || object.touched(f.blockStarts[2], f.blockStarts[3]) || budget.Snapshot() != before {
				t.Fatalf("page allocated/read before atomic rejection: record=%+v err=%v usage=%+v", record, err, budget.Snapshot())
			}
		})
	}
}

func TestFileSourceEncodingConcurrentCacheHitsShareBudget(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 1, 1, 1)
	budget := resource.New(resource.DefaultLimits())
	source, object, _ := encodingBudgetSource(f)
	index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget})
	if err != nil {
		t.Fatal(err)
	}
	defer closeIndex()
	// Concurrent cold lookups must serialize each directory/page miss. Compare
	// their actual costs to one independent query taking the sequential path.
	var group sync.WaitGroup
	for i := 0; i < 32; i++ {
		group.Add(1)
		go func() {
			defer group.Done()
			if _, err := index.FindContent(ctx, f.contentKey); err != nil {
				t.Error(err)
			}
			if _, err := index.FindEncoding(ctx, f.encodingKey); err != nil {
				t.Error(err)
			}
		}()
	}
	group.Wait()
	wantBudget := resource.New(resource.DefaultLimits())
	wantSource, wantObject, _ := encodingBudgetSource(f)
	want, closeWant, err := wantSource.encodingIndex(ctx, FileQuery{budget: wantBudget})
	if err != nil {
		t.Fatal(err)
	}
	defer closeWant()
	if _, err := want.FindContent(ctx, f.contentKey); err != nil {
		t.Fatal(err)
	}
	if _, err := want.FindEncoding(ctx, f.encodingKey); err != nil {
		t.Fatal(err)
	}
	if budget.Snapshot() != wantBudget.Snapshot() || len(object.reads) != len(wantObject.reads) {
		t.Fatalf("concurrent misses duplicated reads/accounting: got=%+v reads=%d want=%+v reads=%d", budget.Snapshot(), len(object.reads), wantBudget.Snapshot(), len(wantObject.reads))
	}
	// Preparing another authenticated source retains the same query budget;
	// it cannot borrow the first source's cached pages or reset its counters.
	before := budget.Snapshot()
	otherSource, _, _ := encodingBudgetSource(f)
	other, closeOther, err := otherSource.encodingIndex(ctx, FileQuery{budget: budget})
	if err != nil {
		t.Fatal(err)
	}
	defer closeOther()
	if _, err := other.FindContent(ctx, f.contentKey); err != nil {
		t.Fatal(err)
	}
	if _, err := other.FindEncoding(ctx, f.encodingKey); err != nil {
		t.Fatal(err)
	}
	after := budget.Snapshot()
	if after.MetadataBytes != before.MetadataBytes*2 || after.RetainedBytes != before.RetainedBytes*2 {
		t.Fatalf("source reset/shared cached allocations: before=%+v after=%+v", before, after)
	}
}

func TestFileSourceEncodingEvictionKeepsCumulativeCharges(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 9, 1, 1024)
	budget := resource.New(resource.DefaultLimits())
	source, object, _ := encodingBudgetSource(f)
	index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget})
	if err != nil {
		t.Fatal(err)
	}
	defer closeIndex()
	for i := 0; i < 9; i++ {
		var key [16]byte
		binary.BigEndian.PutUint32(key[:4], uint32(i+1))
		if _, err := index.FindContent(ctx, hex.EncodeToString(key[:])); err != nil {
			t.Fatal(err)
		}
	}
	before := budget.Snapshot()
	reads := len(object.reads)
	if _, err := index.FindContent(ctx, f.contentKey); err != nil {
		t.Fatal(err)
	}
	after := budget.Snapshot()
	pageCost := int64(1<<20) + encodingCacheEntryOverhead
	if after.MetadataBytes-before.MetadataBytes != pageCost || after.RetainedBytes-before.RetainedBytes != pageCost || len(object.reads) <= reads || index.cache.bytes > encodingCacheCapacity {
		t.Fatalf("eviction refunded old backing bytes or reread was uncharged: before=%+v after=%+v cache=%d", before, after, index.cache.bytes)
	}
	reads = len(object.reads)
	if _, err := index.FindContent(ctx, f.contentKey); err != nil {
		t.Fatal(err)
	}
	if budget.Snapshot() != after || len(object.reads) != reads {
		t.Fatal("warm page was charged/read again")
	}
}

func TestFileSourceEncodingFailedReadsRemainChargedOnRetry(t *testing.T) {
	for _, stage := range []string{"content-page", "encoded-directory"} {
		for _, failure := range []struct {
			name string
			err  error
		}{{"read", io.ErrUnexpectedEOF}, {"cancel", context.Canceled}, {"integrity", container.ErrIntegrity}} {
			t.Run(stage+"/"+failure.name, func(t *testing.T) {
				ctx := context.Background()
				f := makeEncodingBudgetFixture(t, 1, 1, 1)
				budget := resource.New(resource.DefaultLimits())
				source, object, _ := encodingBudgetSource(f)
				index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget})
				if err != nil {
					t.Fatal(err)
				}
				defer closeIndex()
				object.failAt, object.failure = f.blockStarts[2], failure.err
				cost := int64(1024) + encodingCacheEntryOverhead
				lookup := func() error { _, err := index.FindContent(ctx, f.contentKey); return err }
				if stage == "encoded-directory" {
					object.failAt, cost = f.blockStarts[3], 32
					lookup = func() error { _, err := index.FindEncoding(ctx, f.encodingKey); return err }
				}
				before := budget.Snapshot()
				for i := 0; i < 2; i++ {
					if err := lookup(); !errors.Is(err, failure.err) {
						t.Fatalf("failed read lost error: %v", err)
					}
					usage := budget.Snapshot()
					if usage.MetadataBytes-before.MetadataBytes != int64(i+1)*cost || usage.RetainedBytes-before.RetainedBytes != int64(i+1)*cost {
						t.Fatalf("retry refunded or failed to charge actual allocation: before=%+v usage=%+v", before, usage)
					}
				}
				if index.cache.directory != nil || len(index.cache.pages) != 0 {
					t.Fatal("failed read was cached")
				}
				object.failure = nil
				if err := lookup(); err != nil {
					t.Fatalf("retry did not recover original source: %v", err)
				}
			})
		}
	}
}

func TestFileSourceEncodingCancelledWarmLookupDoesNotReadOrCharge(t *testing.T) {
	ctx := context.Background()
	f := makeEncodingBudgetFixture(t, 1, 1, 1)
	budget := resource.New(resource.DefaultLimits())
	source, object, _ := encodingBudgetSource(f)
	index, closeIndex, err := source.encodingIndex(ctx, FileQuery{budget: budget})
	if err != nil {
		t.Fatal(err)
	}
	defer closeIndex()
	if _, err := index.FindContent(ctx, f.contentKey); err != nil {
		t.Fatal(err)
	}
	if _, err := index.FindEncoding(ctx, f.encodingKey); err != nil {
		t.Fatal(err)
	}
	before, reads := budget.Snapshot(), len(object.reads)
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if record, err := index.FindContent(cancelled, f.contentKey); !errors.Is(err, context.Canceled) || record.ContentKey != "" {
		t.Fatalf("cancelled cached content lookup returned data: %+v %v", record, err)
	}
	if record, err := index.FindEncoding(cancelled, f.encodingKey); !errors.Is(err, context.Canceled) || record != (EncodedRecord{}) {
		t.Fatalf("cancelled cached physical lookup returned data: %+v %v", record, err)
	}
	if budget.Snapshot() != before || len(object.reads) != reads || index.cache.directory == nil || len(index.cache.pages) != 2 {
		t.Fatal("cancelled warm lookup read, charged or changed authenticated cache")
	}
}
