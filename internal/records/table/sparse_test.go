package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/table"
)

func sparseFixture(version int, secondary bool) []byte {
	raw, base, start := fixture(version)
	flags := uint16(1)
	if secondary {
		flags |= 2
	}
	binary.LittleEndian.PutUint16(raw[base+36:base+38], flags)
	binary.LittleEndian.PutUint32(raw[base+24:base+28], 10)
	binary.LittleEndian.PutUint32(raw[base+28:base+32], 20)
	section := 72
	if version == 5 {
		section += 132
	}
	raw = append(raw[:start], words(111, 222, 333)...)
	entry := func(offset uint32, length uint16) {
		raw = binary.LittleEndian.AppendUint32(raw, offset)
		raw = binary.LittleEndian.AppendUint16(raw, length)
	}
	if version == 2 {
		binary.LittleEndian.PutUint32(raw[base+28:base+32], 13)
		binary.LittleEndian.PutUint32(raw[section+24:section+28], uint32(start+12))
		binary.LittleEndian.PutUint32(raw[section+20:section+24], 8)
		binary.LittleEndian.PutUint32(raw[section+32:section+36], 20)
		entry(uint32(start), 4)
		entry(0, 0)
		entry(uint32(start+4), 8)
		entry(uint32(start+4), 8)
		raw = append(raw, words(99, 10)...)
		raw = append(raw, words(1, 0, 0, 777, 1)...)
	} else {
		binary.LittleEndian.PutUint32(raw[section+20:section+24], uint32(start+12))
		binary.LittleEndian.PutUint32(raw[section+28:section+32], 20)
		binary.LittleEndian.PutUint32(raw[section+32:section+36], 2)
		binary.LittleEndian.PutUint32(raw[section+36:section+40], 1)
		raw = append(raw, words(99, 10)...)
		entry(uint32(start), 4)
		entry(uint32(start+4), 8)
		ref := uint32(1)
		if secondary && version >= 4 {
			raw = append(raw, words(10, 20)...)
			ref = 20
		}
		raw = append(raw, words(1, 0, 0, 777, ref)...)
		if !secondary || version < 4 {
			raw = append(raw, words(10, 20)...)
		}
	}
	return raw
}

func TestSparseLayoutsAndRelationshipOrder(t *testing.T) {
	for _, version := range []int{2, 3, 4, 5} {
		for _, secondary := range []bool{false, true} {
			raw := sparseFixture(version, secondary)
			r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
			if err != nil {
				t.Fatalf("v%d secondary=%v: %v", version, secondary, err)
			}
			second := uint32(20)
			if version == 2 {
				second = 12
			}
			for id, length := range map[uint32]int{10: 4, second: 8, 99: 4} {
				got, err := r.Lookup(context.Background(), id)
				if err != nil || len(got.Data) != length {
					t.Fatalf("%+v %v", got, err)
				}
				if id == second && (!got.HasRelation || got.Relation != 777) {
					t.Fatalf("lost relation: %+v", got)
				}
			}
			if version == 2 {
				got, err := r.Lookup(context.Background(), 13)
				if err != nil || got.OriginID != 12 || !got.HasRelation || got.Relation != 777 {
					t.Fatalf("implicit copy: %+v %v", got, err)
				}
			}
			if _, err := r.Values(context.Background(), 10, 0, 1); !errors.Is(err, table.ErrUnsupported) {
				t.Fatal("sparse fields must await ordered schema")
			}
		}
	}
}

func TestSparseMapRejectsBadRangesAndIdentities(t *testing.T) {
	for _, name := range []string{"before", "after", "zero-size", "overlap", "duplicate-id", "mismatched-count", "bad-relation", "truncated"} {
		t.Run(name, func(t *testing.T) {
			raw := sparseFixture(3, false)
			_, _, start := fixture(3)
			mapStart := start + 12 + 8
			switch name {
			case "before":
				binary.LittleEndian.PutUint32(raw[mapStart:mapStart+4], 1)
			case "after":
				binary.LittleEndian.PutUint32(raw[mapStart:mapStart+4], uint32(start+10))
			case "zero-size":
				binary.LittleEndian.PutUint16(raw[mapStart+4:mapStart+6], 0)
			case "overlap":
				binary.LittleEndian.PutUint32(raw[mapStart+6:mapStart+10], uint32(start+2))
			case "duplicate-id":
				copy(raw[len(raw)-4:], raw[len(raw)-8:len(raw)-4])
			case "mismatched-count":
				binary.LittleEndian.PutUint32(raw[104:108], 1)
			case "bad-relation":
				binary.LittleEndian.PutUint32(raw[len(raw)-12:len(raw)-8], 99)
			case "truncated":
				raw = raw[:len(raw)-1]
			}
			if got, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits); err == nil || got != nil {
				t.Fatalf("accepted %s", name)
			}
		})
	}
}

func FuzzSparseIndex(f *testing.F) {
	for _, version := range []int{2, 3, 4, 5} {
		f.Add(sparseFixture(version, false))
		f.Add(sparseFixture(version, true))
	}
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 1<<20 {
			t.Skip()
		}
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err == nil {
			_, _ = r.Lookup(context.Background(), 10)
		}
	})
}
