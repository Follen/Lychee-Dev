package records

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"reflect"
	"testing"
)

// Two IDs per group, with the same FDIDs in a second distinct locale/variant.
// The sequential public parser is the reference for the compact directory.
func indexedRootFixture(version int, nameless bool) []byte {
	var raw []byte
	word := func(n uint32) { raw = binary.LittleEndian.AppendUint32(raw, n) }
	if version >= 0 {
		raw = append(raw, "TSFM"...)
		if version == 0 {
			word(4)
			if nameless {
				word(0)
			} else {
				word(4)
			}
		} else {
			word(24)
			word(uint32(version))
			word(4)
			if nameless {
				word(0)
			} else {
				word(4)
			}
			word(0)
		}
	}
	for group := range 2 {
		word(2)
		flags, locale := uint32(0x80), uint32(0x202)
		if nameless {
			flags |= 0x10000000
		}
		if group == 1 {
			flags |= 0x200
			locale = 0x10
		}
		if version == 2 {
			word(locale)
			word(flags)
			word(0x20)
			raw = append(raw, 1)
		} else {
			word(flags)
			word(locale)
		}
		word(11)
		word(7)
		for _, key := range []byte{1, 2} {
			raw = append(raw, bytes.Repeat([]byte{key + byte(group)}, 16)...)
			if version < 0 {
				raw = binary.LittleEndian.AppendUint64(raw, uint64(key)+100)
			}
		}
		if version >= 0 && !nameless {
			raw = binary.LittleEndian.AppendUint64(raw, 101)
			raw = binary.LittleEndian.AppendUint64(raw, 102)
		}
	}
	return raw
}

func TestRootIndexPreservesAllLayoutsVariantsAndNames(t *testing.T) {
	ctx := context.Background()
	limits := RootLimits{Bytes: 1 << 20, Records: 100, Groups: 10, Matches: 100}
	for _, version := range []int{-1, 0, 1, 2} {
		for _, nameless := range []bool{false, true} {
			if version < 0 && nameless {
				continue
			}
			raw := indexedRootFixture(version, nameless)
			index, err := newRootIndex(ctx, bytes.NewReader(raw), int64(len(raw)), limits)
			if err != nil {
				t.Fatal(version, nameless, err)
			}
			for _, id := range []uint32{0, 11, 18, 19, 20, ^uint32(0)} {
				want, err := LookupRoot(ctx, bytes.NewReader(raw), int64(len(raw)), []uint32{id}, limits)
				if err != nil {
					t.Fatal(err)
				}
				got, err := index.lookup(ctx, id)
				if err != nil || !reflect.DeepEqual(got, want) {
					t.Fatalf("version=%d nameless=%v id=%d: %+v want %+v err=%v", version, nameless, id, got, want, err)
				}
			}
			index.limits.Matches = 1
			if got, err := index.lookup(ctx, 11); !errors.Is(err, ErrMetadataLimit) || got != nil {
				t.Fatal("lost duplicate-variant limit", got, err)
			}
			canceled, cancel := context.WithCancel(ctx)
			cancel()
			if got, err := index.lookup(canceled, 11); !errors.Is(err, context.Canceled) || got != nil {
				t.Fatal(got, err)
			}
		}
	}
}
