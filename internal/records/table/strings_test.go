package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/table"
)

func stringFixture(version int, text []byte, pointers ...uint32) []byte {
	raw, _, start := fixture(version)
	section := 72
	if version == 5 {
		section += 132
	}
	binary.LittleEndian.PutUint32(raw[section+16:section+20], uint32(len(text)))
	// External IDs let the one stored column be a pointer rather than the ID.
	idOffset := section + 24
	if version == 2 {
		idOffset = section + 28
	}
	binary.LittleEndian.PutUint32(raw[idOffset:idOffset+4], 8)
	for i, p := range pointers {
		binary.LittleEndian.PutUint32(raw[start+i*4:start+i*4+4], p)
	}
	raw = append(raw, text...)
	return append(raw, words(1, 2)...)
}

func TestStringsRelativePointersAndCopy(t *testing.T) {
	for _, version := range []int{2, 3, 4, 5} {
		text := []byte("hello\x00中文\x00")
		raw := stringFixture(version, text, 8, 10)
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			t.Fatal(err)
		}
		for id, want := range map[uint32]string{1: "hello", 2: "中文"} {
			got, err := r.Strings(context.Background(), id, 0, 1, len(want))
			if err != nil || !reflect.DeepEqual(got, []string{want}) {
				t.Fatalf("v%d: %v %v", version, got, err)
			}
		}
	}
	raw := stringFixture(3, []byte("copy\x00"), 8, 0)
	binary.LittleEndian.PutUint32(raw[108:112], 1)
	raw = append(raw, words(99, 1)...)
	r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	got, err := r.Strings(context.Background(), 99, 0, 1, 4)
	if err != nil || got[0] != "copy" {
		t.Fatalf("%v %v", got, err)
	}
}

func TestStringRejectsUnterminatedInvalidAndOversizedContent(t *testing.T) {
	for _, test := range []struct {
		text    []byte
		pointer uint32
		budget  int
		want    error
	}{
		{[]byte("abc"), 8, 10, table.ErrFormat},
		{[]byte("abc\x00"), 8, 2, table.ErrLimit},
		{[]byte{0xff, 0}, 8, 10, table.ErrFormat},
		{[]byte("abc\x00"), 0xffffffff, 10, table.ErrFormat},
		{[]byte("abc\x00"), 12, 10, table.ErrFormat},
	} {
		raw := stringFixture(3, test.text, test.pointer, 0)
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			t.Fatal(err)
		}
		got, err := r.Strings(context.Background(), 1, 0, 1, test.budget)
		if !errors.Is(err, test.want) || got != nil {
			t.Fatalf("%v %v want %v", got, err, test.want)
		}
	}
}

func TestStringCrossPartitionVirtualOffsets(t *testing.T) {
	// Two 4-byte records and concatenated logical strings "first\0second\0".
	// Physical auxiliary ID lists between strings must not affect addressing.
	raw := make([]byte, 217)
	base, _, _ := fixture(3)
	copy(raw[:72], base[:72])
	binary.LittleEndian.PutUint32(raw[68:72], 2)
	for i, offset := range []uint32{180, 197} {
		section := 72 + i*40
		binary.LittleEndian.PutUint32(raw[section+8:section+12], offset)
		binary.LittleEndian.PutUint32(raw[section+12:section+16], 1)
		binary.LittleEndian.PutUint32(raw[section+16:section+20], 7)
		binary.LittleEndian.PutUint32(raw[section+24:section+28], 4)
	}
	// Metadata: two partition headers, one placement, one storage descriptor.
	copy(raw[156:180], base[116:140])
	binary.LittleEndian.PutUint32(raw[180:184], 15) // first record -> second string base 7
	copy(raw[184:191], []byte("first\x00\x00"))
	binary.LittleEndian.PutUint32(raw[191:195], 1)
	binary.LittleEndian.PutUint32(raw[197:201], 4) // second record -> first string base 0
	copy(raw[201:208], []byte("second\x00"))
	binary.LittleEndian.PutUint32(raw[208:212], 2)
	r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	for id, want := range map[uint32]string{1: "second", 2: "first"} {
		got, err := r.Strings(context.Background(), id, 0, 1, 100)
		if err != nil || got[0] != want {
			t.Fatalf("%v %v", got, err)
		}
	}
}

func TestStringArrayBudgetAndCancellation(t *testing.T) {
	raw, _, start := fixture(3)
	binary.LittleEndian.PutUint32(raw[12:16], 8)
	binary.LittleEndian.PutUint32(raw[88:92], 4)
	binary.LittleEndian.PutUint32(raw[96:100], 8)
	binary.LittleEndian.PutUint16(raw[start-22:start-20], 64)
	raw = append(raw[:start], make([]byte, 16)...)
	binary.LittleEndian.PutUint32(raw[start:start+4], 16)
	binary.LittleEndian.PutUint32(raw[start+4:start+8], 14)
	raw = append(raw, []byte("a\x00b\x00")...)
	raw = append(raw, words(1, 2)...)
	r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	got, err := r.Strings(context.Background(), 1, 0, 2, 2)
	if err != nil || !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Fatalf("%v %v", got, err)
	}
	if got, err := r.Strings(context.Background(), 1, 0, 2, 1); !errors.Is(err, table.ErrLimit) || got != nil {
		t.Fatalf("partial array: %v %v", got, err)
	}
	got, err = r.Strings(context.Background(), 2, 0, 2, 1)
	if err != nil || !reflect.DeepEqual(got, []string{"", ""}) {
		t.Fatalf("zero pointers: %v %v", got, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := r.Strings(ctx, 1, 0, 2, 2); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func FuzzStringReferences(f *testing.F) {
	f.Add([]byte("hello\x00"), uint32(8))
	f.Fuzz(func(t *testing.T, text []byte, pointer uint32) {
		if len(text) > 1024 {
			t.Skip()
		}
		raw := stringFixture(3, text, pointer, 0)
		r, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = r.Strings(context.Background(), 1, 0, 1, 256)
	})
}
