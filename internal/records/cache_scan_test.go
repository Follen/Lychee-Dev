package records_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
)

type cacheRecordFixture struct {
	Region   uint32
	UniqueID uint32
	Push     int32
	Table    uint32
	Record   uint32
	Status   uint8
	Payload  []byte
}

// makeCacheSnapshot writes the XFTH layouts directly from their documented
// offsets. It intentionally does not use the records package's parser.
func makeCacheSnapshot(version, build, layout uint32, fixtures ...cacheRecordFixture) []byte {
	headerBytes := 12
	if version >= 5 {
		headerBytes = 44
	}
	raw := make([]byte, headerBytes)
	copy(raw[:4], "XFTH")
	binary.LittleEndian.PutUint32(raw[4:8], version)
	binary.LittleEndian.PutUint32(raw[8:12], build)

	for _, fixture := range fixtures {
		width := cacheRecordWidth(layout)
		header := make([]byte, width)
		copy(header[:4], "XFTH")
		switch layout {
		case 1:
			putCacheU32(header, 4, uint32(fixture.Push))
			putCacheU32(header, 8, uint32(len(fixture.Payload)))
			putCacheU32(header, 12, fixture.Table)
			putCacheU32(header, 16, fixture.Record)
			header[20] = fixture.Status
		case 2, 3, 4, 5, 6:
			putCacheU32(header, 8, uint32(fixture.Push))
			putCacheU32(header, 12, uint32(len(fixture.Payload)))
			putCacheU32(header, 16, fixture.Table)
			putCacheU32(header, 20, fixture.Record)
			header[24] = fixture.Status
		case 7:
			putCacheU32(header, 4, uint32(fixture.Push))
			putCacheU32(header, 8, fixture.Table)
			putCacheU32(header, 12, fixture.Record)
			putCacheU32(header, 16, uint32(len(fixture.Payload)))
			header[20] = fixture.Status
		case 8:
			putCacheU32(header, 4, uint32(fixture.Push))
			putCacheU32(header, 8, fixture.UniqueID)
			putCacheU32(header, 12, fixture.Table)
			putCacheU32(header, 16, fixture.Record)
			putCacheU32(header, 20, uint32(len(fixture.Payload)))
			header[24] = fixture.Status
		case 9:
			putCacheU32(header, 4, fixture.Region)
			putCacheU32(header, 8, uint32(fixture.Push))
			putCacheU32(header, 12, fixture.UniqueID)
			putCacheU32(header, 16, fixture.Table)
			putCacheU32(header, 20, fixture.Record)
			putCacheU32(header, 24, uint32(len(fixture.Payload)))
			header[28] = fixture.Status
		default:
			panic("unsupported cache fixture layout")
		}
		raw = append(raw, header...)
		raw = append(raw, fixture.Payload...)
	}
	return raw
}

func putCacheU32(dst []byte, offset int, value uint32) {
	binary.LittleEndian.PutUint32(dst[offset:offset+4], value)
}

func cacheRecordWidth(layout uint32) int {
	if layout == 1 || layout == 7 {
		return 24
	}
	if layout == 9 {
		return 32
	}
	return 28
}

func cacheDataOffset(version uint32) int {
	if version >= 5 {
		return 44
	}
	return 12
}

func cacheU32(value uint32) *uint32 {
	return &value
}

func TestInspectCacheVersions(t *testing.T) {
	const build = 120100
	fixture := cacheRecordFixture{
		Region:   0x01020304,
		UniqueID: 0xaabbccdd,
		Push:     -7,
		Table:    0x11223344,
		Record:   0x55667788,
		Status:   0xf3,
		Payload:  []byte{0xde, 0xad},
	}
	cases := []struct {
		name    string
		version uint32
		layout  uint32
	}{
		{name: "v1", version: 1, layout: 1},
		{name: "v2", version: 2, layout: 2},
		{name: "v3", version: 3, layout: 3},
		{name: "v4", version: 4, layout: 4},
		{name: "v5", version: 5, layout: 5},
		{name: "v6", version: 6, layout: 6},
		{name: "v7", version: 7, layout: 7},
		{name: "v8-framed-as-v7", version: 8, layout: 7},
		{name: "v8", version: 8, layout: 8},
		{name: "v9", version: 9, layout: 9},
	}

	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			raw := makeCacheSnapshot(test.version, build, test.layout, fixture)
			page, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
				Build: build,
				Limit: 1,
			})
			if err != nil {
				t.Fatal(err)
			}
			if page.Version != test.version || page.RecordLayout != test.layout {
				t.Fatalf("format: got version=%d layout=%d, want version=%d layout=%d", page.Version, page.RecordLayout, test.version, test.layout)
			}
			if page.Scanned != 1 || page.Matched != 1 || !page.Complete || page.Truncated || len(page.Entries) != 1 {
				t.Fatalf("page summary: %#v", page)
			}
			entry := page.Entries[0]
			if entry.Index != 0 || entry.Push != fixture.Push || entry.TableHash != fixture.Table || entry.RecordID != fixture.Record || entry.Status != fixture.Status || entry.PayloadBytes != len(fixture.Payload) || entry.PayloadHex != "dead" {
				t.Fatalf("entry: %#v", entry)
			}
			wantOffset := int64(cacheDataOffset(test.version) + cacheRecordWidth(test.layout))
			if entry.PayloadOffset != wantOffset {
				t.Fatalf("payload offset: got %d want %d", entry.PayloadOffset, wantOffset)
			}
			if test.layout == 9 {
				if entry.Region == nil || *entry.Region != fixture.Region || entry.UniqueID == nil || *entry.UniqueID != fixture.UniqueID {
					t.Fatalf("v9 identity fields: %#v", entry)
				}
			} else if test.layout == 8 {
				if entry.Region != nil || entry.UniqueID == nil || *entry.UniqueID != fixture.UniqueID {
					t.Fatalf("v8 identity fields: %#v", entry)
				}
			} else if entry.Region != nil || entry.UniqueID != nil {
				t.Fatalf("unexpected identity fields: %#v", entry)
			}
		})
	}
}

func TestInspectCachePreservesPhysicalEntriesAndPagination(t *testing.T) {
	const build = 120100
	raw := makeCacheSnapshot(9, build, 9,
		cacheRecordFixture{Push: 1, Table: 7, Record: 42, Status: 1, Payload: []byte("a")},
		cacheRecordFixture{Push: -2, Table: 7, Record: 42, Status: 0xfe, Payload: []byte("b")},
		cacheRecordFixture{Push: 9, Table: 7, Record: 99, Status: 2, Payload: []byte("c")},
		cacheRecordFixture{Push: 3, Table: 7, Record: 42, Status: 3, Payload: []byte("d")},
	)

	page, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:      build,
		RecordID:   cacheU32(42),
		AfterIndex: cacheU32(0),
		Limit:      1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if page.Scanned != 4 || page.Matched != 3 || !page.Truncated || page.Complete || len(page.Entries) != 1 {
		t.Fatalf("page summary: %#v", page)
	}
	entry := page.Entries[0]
	if entry.Index != 1 || entry.RecordID != 42 || entry.Push != -2 || entry.Status != 0xfe || entry.PayloadHex != "62" {
		t.Fatalf("physical entry was not preserved: %#v", entry)
	}
	if page.NextIndex == nil || *page.NextIndex != 1 {
		t.Fatalf("next index: %#v", page.NextIndex)
	}

	next, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:      build,
		RecordID:   cacheU32(42),
		AfterIndex: cacheU32(1),
		Limit:      10,
	})
	if err != nil {
		t.Fatal(err)
	}
	if next.Scanned != 4 || next.Matched != 3 || next.Truncated || next.Complete || len(next.Entries) != 1 || next.Entries[0].Index != 3 || next.Entries[0].PayloadHex != "64" {
		t.Fatalf("second page: %#v", next)
	}
}

func TestInspectCacheLatestSelectsWholeSignedPushBatch(t *testing.T) {
	const build = 120100
	const table = 7
	const record = 42
	raw := makeCacheSnapshot(9, build, 9,
		cacheRecordFixture{Push: 8, Table: table, Record: record, Status: 1, Payload: []byte("a")},
		cacheRecordFixture{Push: 8, Table: table, Record: record, Status: 0xfe, Payload: []byte("b")},
		cacheRecordFixture{Push: 3, Table: table, Record: record, Status: 2, Payload: []byte("c")},
		cacheRecordFixture{Push: 8, Table: table, Record: record, Status: 3, Payload: []byte("d")},
		cacheRecordFixture{Push: 99, Table: table + 1, Record: record, Status: 4, Payload: []byte("e")},
		cacheRecordFixture{Push: 7, Table: table, Record: record, Status: 5, Payload: []byte("f")},
	)

	page, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:     build,
		TableHash: cacheU32(table),
		RecordID:  cacheU32(record),
		Latest:    true,
		Limit:     2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if page.Scanned != 6 || page.Matched != 3 || page.SelectedPush == nil || *page.SelectedPush != 8 || !page.Truncated || page.Complete || len(page.Entries) != 2 {
		t.Fatalf("latest page summary: %#v", page)
	}
	if page.Entries[0].Index != 0 || page.Entries[1].Index != 1 || page.Entries[1].Status != 0xfe {
		t.Fatalf("latest physical page: %#v", page.Entries)
	}
	if page.NextIndex == nil || *page.NextIndex != 1 {
		t.Fatalf("latest next index: %#v", page.NextIndex)
	}

	next, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:      build,
		TableHash:  cacheU32(table),
		RecordID:   cacheU32(record),
		Latest:     true,
		AfterIndex: cacheU32(1),
		Limit:      10,
	})
	if err != nil {
		t.Fatal(err)
	}
	if next.Scanned != 6 || next.Matched != 3 || next.SelectedPush == nil || *next.SelectedPush != 8 || next.Truncated || len(next.Entries) != 1 || next.Entries[0].Index != 3 {
		t.Fatalf("latest second page: %#v", next)
	}
}

func TestInspectCacheLatestHandlesNegativePushNoMatchAndCursor(t *testing.T) {
	const build = 120100
	raw := makeCacheSnapshot(9, build, 9,
		cacheRecordFixture{Push: -10, Table: 7, Record: 42, Payload: []byte("a")},
		cacheRecordFixture{Push: -2, Table: 7, Record: 42, Status: 1, Payload: []byte("b")},
		cacheRecordFixture{Push: -2, Table: 7, Record: 42, Status: 2, Payload: []byte("c")},
	)

	page, err := records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:      build,
		TableHash:  cacheU32(7),
		RecordID:   cacheU32(42),
		Latest:     true,
		AfterIndex: cacheU32(99),
		Limit:      1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if page.Scanned != 3 || page.Matched != 2 || page.SelectedPush == nil || *page.SelectedPush != -2 || page.Truncated || len(page.Entries) != 0 {
		t.Fatalf("cursor beyond latest batch: %#v", page)
	}

	page, err = records.InspectCache(context.Background(), raw, records.CacheFilter{
		Build:     build,
		TableHash: cacheU32(99),
		Latest:    true,
		Limit:     1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if page.Scanned != 3 || page.Matched != 0 || page.SelectedPush != nil || len(page.Entries) != 0 {
		t.Fatalf("latest no-match page: %#v", page)
	}
}

func TestInspectCacheLatestValidatesTailAndContext(t *testing.T) {
	const build = 120100
	raw := makeCacheSnapshot(9, build, 9, cacheRecordFixture{Push: 4, Table: 7, Record: 42, Payload: []byte("ok")})
	badTail := append(bytes.Clone(raw), 0x58, 0x46)
	_, err := records.InspectCache(context.Background(), badTail, records.CacheFilter{
		Build:  build,
		Latest: true,
		Limit:  1,
	})
	if !errors.Is(err, records.ErrCacheFormat) {
		t.Fatalf("bad latest tail: got %v, want ErrCacheFormat", err)
	}

	ctx := &cancelAfterChecks{cancelAt: 3}
	_, err = records.InspectCache(ctx, raw, records.CacheFilter{Build: build, Latest: true, Limit: 1})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("latest cancellation: got %v, want context.Canceled", err)
	}
}

func TestInspectCacheRejectsCorruptTailAfterPagingFilters(t *testing.T) {
	const build = 120100
	const table = 7
	filter := records.CacheFilter{Build: build, TableHash: cacheU32(table), Limit: 1}

	t.Run("corrupt header", func(t *testing.T) {
		raw := makeCacheSnapshot(9, build, 9,
			cacheRecordFixture{Table: table, Record: 1, Payload: []byte("ok")},
			cacheRecordFixture{Table: 99, Record: 2, Payload: []byte("bad")},
		)
		second := cacheDataOffset(9) + cacheRecordWidth(9) + 2
		copy(raw[second:second+4], "BAD!")
		_, err := records.InspectCache(context.Background(), raw, filter)
		if !errors.Is(err, records.ErrCacheFormat) {
			t.Fatalf("error: got %v, want ErrCacheFormat", err)
		}
	})

	t.Run("truncated tail", func(t *testing.T) {
		raw := makeCacheSnapshot(9, build, 9, cacheRecordFixture{Table: table, Record: 1, Payload: []byte("ok")})
		raw = append(raw, 0x58, 0x46)
		_, err := records.InspectCache(context.Background(), raw, filter)
		if !errors.Is(err, records.ErrCacheFormat) {
			t.Fatalf("error: got %v, want ErrCacheFormat", err)
		}
	})
}

func TestInspectCacheRejectsLimitsAndPayloadOverflow(t *testing.T) {
	raw := makeCacheSnapshot(9, 120100, 9, cacheRecordFixture{Table: 7, Record: 1, Payload: []byte("ok")})
	for _, limit := range []int{-1, 0, 201} {
		t.Run("limit", func(t *testing.T) {
			_, err := records.InspectCache(context.Background(), raw, records.CacheFilter{Build: 120100, Limit: limit})
			if !errors.Is(err, records.ErrCacheLimit) {
				t.Fatalf("limit %d: got %v, want ErrCacheLimit", limit, err)
			}
		})
	}

	tooLarge := bytes.Repeat([]byte{0xab}, 8<<20+1)
	raw = makeCacheSnapshot(9, 120100, 9, cacheRecordFixture{Table: 7, Record: 1, Payload: tooLarge})
	_, err := records.InspectCache(context.Background(), raw, records.CacheFilter{Build: 120100, Limit: 1})
	if !errors.Is(err, records.ErrCacheLimit) {
		t.Fatalf("payload budget: got %v, want ErrCacheLimit", err)
	}
}

func TestInspectCacheRejectsBuildMismatchAndCancellation(t *testing.T) {
	raw := makeCacheSnapshot(9, 120100, 9, cacheRecordFixture{Table: 7, Record: 1, Payload: []byte("ok")})
	_, err := records.InspectCache(context.Background(), raw, records.CacheFilter{Build: 120101, Limit: 1})
	if !errors.Is(err, records.ErrCacheBuild) {
		t.Fatalf("build mismatch: got %v, want ErrCacheBuild", err)
	}

	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = records.InspectCache(canceled, raw, records.CacheFilter{Build: 120100, Limit: 1})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("pre-canceled context: got %v, want context.Canceled", err)
	}

	ctx := &cancelAfterChecks{cancelAt: 3}
	raw = makeCacheSnapshot(9, 120100, 9,
		cacheRecordFixture{Table: 7, Record: 1, Payload: []byte("a")},
		cacheRecordFixture{Table: 7, Record: 2, Payload: []byte("b")},
	)
	_, err = records.InspectCache(ctx, raw, records.CacheFilter{Build: 120100, Limit: 10})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled during scan: got %v, want context.Canceled", err)
	}

	oneScan := &cancelAfterChecks{cancelAt: 5}
	page, err := records.InspectCache(oneScan, raw, records.CacheFilter{Build: 120100, Limit: 10})
	if err != nil || page.Scanned != 2 || page.Matched != 2 {
		t.Fatalf("ordinary query should use one scan: page=%#v err=%v", page, err)
	}
}

type cancelAfterChecks struct {
	checks   int
	cancelAt int
}

func (c *cancelAfterChecks) Deadline() (time.Time, bool) { return time.Time{}, false }
func (c *cancelAfterChecks) Done() <-chan struct{}       { return nil }
func (c *cancelAfterChecks) Value(any) any               { return nil }

func (c *cancelAfterChecks) Err() error {
	c.checks++
	if c.checks >= c.cancelAt {
		return context.Canceled
	}
	return nil
}

func FuzzInspectCache(f *testing.F) {
	f.Add(makeCacheSnapshot(9, 120100, 9, cacheRecordFixture{Table: 7, Record: 1, Payload: []byte("seed")}), uint32(120100), 1)
	f.Add([]byte("XFTH"), uint32(1), 1)
	f.Add([]byte{}, uint32(1), 1)

	f.Fuzz(func(t *testing.T, raw []byte, build uint32, limit int) {
		if len(raw) > 2<<20 {
			raw = raw[:2<<20]
		}
		_, _ = records.InspectCache(context.Background(), raw, records.CacheFilter{
			Build: build,
			Limit: limit,
		})
	})
}

func TestInspectCacheRejectsInvalidFraming(t *testing.T) {
	base := makeCacheSnapshot(9, 69875, 9, cacheRecordFixture{Table: 7, Record: 1, Payload: []byte{1, 2}})
	for _, tc := range []struct {
		name string
		raw  []byte
		want error
	}{
		{"short-file", []byte("XFTH"), records.ErrCacheFormat},
		{"short-extended", base[:43], records.ErrCacheFormat},
		{"short-payload", base[:len(base)-1], records.ErrCacheFormat},
		{"unknown-version", func() []byte { r := bytes.Clone(base); putCacheU32(r, 4, 10); return r }(), records.ErrCacheVersion},
		{"negative-size", func() []byte { r := bytes.Clone(base); putCacheU32(r, 44+24, 0xffffffff); return r }(), records.ErrCacheFormat},
		{"zero-build", func() []byte { r := bytes.Clone(base); putCacheU32(r, 8, 0); return r }(), records.ErrCacheBuild},
		// A v8 record with ID 4 and empty payload is also a complete v7
		// record with four payload bytes. Neither interpretation is privileged.
		{"ambiguous-v8", makeCacheSnapshot(8, 69875, 8, cacheRecordFixture{Record: 4}), records.ErrCacheFormat},
	} {
		t.Run(tc.name, func(t *testing.T) {
			page, err := records.InspectCache(context.Background(), tc.raw, records.CacheFilter{Build: 69875, Limit: 1})
			if !errors.Is(err, tc.want) || len(page.Entries) != 0 {
				t.Fatal(page, err)
			}
		})
	}
	for _, version := range []uint32{1, 8, 9} {
		page, err := records.InspectCache(context.Background(), makeCacheSnapshot(version, 69875, version), records.CacheFilter{Build: 69875, Limit: 1})
		if err != nil || !page.Complete || page.Truncated || page.Scanned != 0 || page.Entries == nil {
			t.Fatal(page, err)
		}
	}
}
