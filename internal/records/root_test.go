package records_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

var rootTestLimits = records.RootLimits{Bytes: 1 << 20, Records: 10000, Groups: 100, Matches: 100}

func rootFixture(version int, nameless bool) []byte {
	var out []byte
	word := func(v uint32) { out = binary.LittleEndian.AppendUint32(out, v) }
	if version >= 0 {
		out = append(out, "TSFM"...)
		if version == 0 {
			word(2)
			if nameless {
				word(0)
			} else {
				word(2)
			}
		} else {
			word(24)
			word(uint32(version))
			word(2)
			if nameless {
				word(0)
			} else {
				word(2)
			}
			word(0)
		}
	}
	word(2)
	flags := uint32(0x80)
	if nameless {
		flags |= 0x10000000
	}
	if version == 2 {
		word(0x202)
		word(flags)
		word(0x20)
		out = append(out, 1)
	} else {
		word(flags)
		word(0x202)
	}
	word(11)
	word(7) // IDs 11 and 19, not 11 and 18.
	for _, b := range []byte{1, 2} {
		out = append(out, bytes.Repeat([]byte{b}, 16)...)
		if version < 0 {
			out = binary.LittleEndian.AppendUint64(out, uint64(b)+100)
		}
	}
	if version >= 0 && !nameless {
		out = binary.LittleEndian.AppendUint64(out, 101)
		out = binary.LittleEndian.AppendUint64(out, 102)
	}
	return out
}

func TestRootLayoutsRetainVariantsAndNames(t *testing.T) {
	for _, version := range []int{-1, 0, 1, 2} {
		for _, nameless := range []bool{false, true} {
			if version < 0 && nameless {
				continue
			}
			raw := rootFixture(version, nameless)
			matches, err := records.LookupRoot(context.Background(), bytes.NewReader(raw), int64(len(raw)), []uint32{19, 11, 19}, rootTestLimits)
			if err != nil {
				t.Fatalf("version=%d nameless=%v: %v", version, nameless, err)
			}
			if len(matches) != 2 || matches[0].FileDataID != 11 || matches[1].FileDataID != 19 {
				t.Fatalf("%+v", matches)
			}
			for i, m := range matches {
				if m.LocaleMask != 0x202 || m.ContentFlags&0x80 == 0 || m.HasNameHash == nameless {
					t.Fatalf("lost variant: %+v", m)
				}
				if !nameless && m.NameHash != uint64(101+i) {
					t.Fatalf("bad name hash: %+v", m)
				}
				if version == 2 && m.ContentFlags&0x20020 != 0x20020 {
					t.Fatalf("lost v2 flags: %+v", m)
				}
			}
		}
	}
}

func TestRootRejectsTruncationOverflowAndLimitsWithoutPartialResults(t *testing.T) {
	raw := rootFixture(2, false)
	// Prefixes including a complete header alone are structurally empty roots.
	for n := 1; n < len(raw); n++ {
		if n == 24 {
			continue
		}
		matches, err := records.LookupRoot(context.Background(), bytes.NewReader(raw[:n]), int64(n), []uint32{11}, rootTestLimits)
		if err == nil || len(matches) != 0 {
			t.Fatalf("accepted prefix %d: %+v %v", n, matches, err)
		}
	}
	broken := append(append([]byte{}, raw...), 1)
	if got, err := records.LookupRoot(context.Background(), bytes.NewReader(broken), int64(len(broken)), []uint32{11}, rootTestLimits); err == nil || len(got) != 0 {
		t.Fatalf("partial results: %v %v", got, err)
	}
	broken = append([]byte{}, raw...)
	binary.LittleEndian.PutUint32(broken[41:45], 0xffffffff)
	if got, err := records.LookupRoot(context.Background(), bytes.NewReader(broken), int64(len(broken)), []uint32{0xffffffff}, rootTestLimits); !errors.Is(err, records.ErrMetadataFormat) || len(got) != 0 {
		t.Fatalf("delta overflow: %v %v", got, err)
	}
	for _, kind := range []string{"bytes", "records", "matches", "groups"} {
		limits := rootTestLimits
		input := raw
		switch kind {
		case "bytes":
			limits.Bytes = int64(len(raw) - 1)
		case "records":
			limits.Records = 1
		case "matches":
			limits.Matches = 1
		case "groups":
			limits.Groups = 1
			input = append(append([]byte{}, raw...), raw[24:]...)
		}
		if got, err := records.LookupRoot(context.Background(), bytes.NewReader(input), int64(len(input)), []uint32{11, 19}, limits); !errors.Is(err, records.ErrMetadataLimit) || len(got) != 0 {
			t.Fatalf("%s: %v %v", kind, got, err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := records.LookupRoot(ctx, bytes.NewReader(raw), int64(len(raw)), []uint32{11}, rootTestLimits); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestRootSameIDAcrossLocalesIsNotCollapsed(t *testing.T) {
	raw := rootFixture(-1, false)
	second := append([]byte{}, raw...)
	binary.LittleEndian.PutUint32(second[8:12], 0x10)
	raw = append(raw, second...)
	got, err := records.LookupRoot(context.Background(), bytes.NewReader(raw), int64(len(raw)), []uint32{11}, rootTestLimits)
	if err != nil || len(got) != 2 || got[0].LocaleMask == got[1].LocaleMask || got[1].Group != 1 {
		t.Fatalf("%+v %v", got, err)
	}
}

func TestRootOldHeaderPreservesNamedCount(t *testing.T) {
	raw := rootFixture(0, false)
	// The no-name flag suppresses hashes only when the file header permits it.
	binary.LittleEndian.PutUint32(raw[16:20], 0x10000000)
	got, err := records.LookupRoot(context.Background(), bytes.NewReader(raw), int64(len(raw)), []uint32{11}, rootTestLimits)
	if err != nil || len(got) != 1 || !got[0].HasNameHash || got[0].NameHash != 101 {
		t.Fatalf("%+v %v", got, err)
	}
}

func TestRootOldHeaderSmallCountsAreNotVersions(t *testing.T) {
	raw := rootFixture(0, false)
	binary.LittleEndian.PutUint32(raw[4:8], 24)
	binary.LittleEndian.PutUint32(raw[8:12], 24)
	got, err := records.LookupRoot(context.Background(), bytes.NewReader(raw), int64(len(raw)), []uint32{11}, rootTestLimits)
	if err != nil || len(got) != 1 {
		t.Fatalf("%+v %v", got, err)
	}
}

func FuzzRootLookup(f *testing.F) {
	for _, version := range []int{-1, 0, 1, 2} {
		f.Add(rootFixture(version, false))
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 1<<20 {
			t.Skip()
		}
		_, _ = records.LookupRoot(context.Background(), bytes.NewReader(raw), int64(len(raw)), []uint32{11, 19}, rootTestLimits)
	})
}
