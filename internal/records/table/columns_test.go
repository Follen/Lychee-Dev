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

func columnFixture(codec uint32, offset, width uint16, parameters [3]uint32, aux []byte) []byte {
	raw, base, start := fixture(3)
	binary.LittleEndian.PutUint32(raw[base:base+4], 1)
	binary.LittleEndian.PutUint32(raw[base+8:base+12], 16)
	binary.LittleEndian.PutUint32(raw[84:88], 1)
	binary.LittleEndian.PutUint32(raw[80:84], uint32(start+len(aux)))
	storage := raw[start-24 : start]
	binary.LittleEndian.PutUint16(storage[:2], offset)
	binary.LittleEndian.PutUint16(storage[2:4], width)
	binary.LittleEndian.PutUint32(storage[4:8], uint32(len(aux)))
	binary.LittleEndian.PutUint32(storage[8:12], codec)
	for i, p := range parameters {
		binary.LittleEndian.PutUint32(storage[12+i*4:16+i*4], p)
	}
	if codec == 1 || codec == 3 || codec == 4 || codec == 5 {
		binary.LittleEndian.PutUint32(storage[16:20], uint32(width))
	}
	if codec == 2 {
		binary.LittleEndian.PutUint32(raw[base+56:base+60], uint32(len(aux)))
	} else if codec == 3 || codec == 4 {
		binary.LittleEndian.PutUint32(raw[base+60:base+64], uint32(len(aux)))
	}
	result := append([]byte{}, raw[:start]...)
	result = append(result, aux...)
	return append(result, make([]byte, 16)...)
}

func words(values ...uint32) []byte {
	var out []byte
	for _, v := range values {
		out = binary.LittleEndian.AppendUint32(out, v)
	}
	return out
}
func openColumn(t *testing.T, raw []byte) *table.Columns {
	t.Helper()
	c, err := table.OpenColumns(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestStorageCodecs(t *testing.T) {
	tests := []struct {
		name          string
		codec         uint32
		offset, width uint16
		parameters    [3]uint32
		aux, record   []byte
		id, count     uint32
		want          []uint64
	}{
		{"direct-array", 0, 0, 32, [3]uint32{}, nil, []byte{1, 2, 3, 4}, 0, 4, []uint64{1, 2, 3, 4}},
		{"packed", 1, 3, 9, [3]uint32{}, nil, []byte{0xf8, 0x0f}, 0, 1, []uint64{511}},
		{"negative", 5, 3, 9, [3]uint32{}, nil, []byte{0xf8, 0x0f}, 0, 1, []uint64{^uint64(0)}},
		{"common-default", 2, 0, 0, [3]uint32{33}, words(7, 99), nil, 1, 1, []uint64{33}},
		{"common-override", 2, 0, 0, [3]uint32{33}, words(7, 99), nil, 7, 1, []uint64{99}},
		{"palette", 3, 0, 2, [3]uint32{}, words(100, 200, 300), []byte{2}, 0, 1, []uint64{300}},
		{"palette-array", 4, 0, 2, [3]uint32{0, 0, 2}, words(1, 2, 3, 4), []byte{1}, 0, 2, []uint64{3, 4}},
		{"zero-bits", 1, 0, 0, [3]uint32{}, nil, nil, 0, 1, []uint64{0}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			c := openColumn(t, columnFixture(tt.codec, tt.offset, tt.width, tt.parameters, tt.aux))
			got, err := c.Values(context.Background(), tt.record, tt.id, 0, tt.count)
			if err != nil || !reflect.DeepEqual(got, tt.want) {
				t.Fatalf("%v %v want %v", got, err, tt.want)
			}
			if len(got) > 0 {
				got[0] = 0
				again, err := c.Values(context.Background(), tt.record, tt.id, 0, tt.count)
				if err != nil || !reflect.DeepEqual(again, tt.want) {
					t.Fatal("mutable output changed compiled data")
				}
			}
		})
	}
}

func TestEveryBitWidthAndAlignment(t *testing.T) {
	for width := uint16(1); width <= 64; width++ {
		for offset := uint16(0); offset < 8; offset++ {
			c := openColumn(t, columnFixture(1, offset, width, [3]uint32{}, nil))
			row := []byte{0x81, 0x42, 0x24, 0x18, 0xaa, 0x55, 0xf0, 0x0f, 0x93}
			var want uint64
			for bit := uint16(0); bit < width; bit++ {
				if row[(offset+bit)/8]&(1<<((offset+bit)%8)) != 0 {
					want |= 1 << bit
				}
			}
			got, err := c.Values(context.Background(), row, 0, 0, 1)
			if err != nil || got[0] != want {
				t.Fatalf("offset %d width %d: %v %v", offset, width, got, err)
			}
		}
	}
}

func TestPackedCodecUsesEncodedWidthParameter(t *testing.T) {
	raw := columnFixture(5, 0, 32, [3]uint32{}, nil)
	// The storage extent and encoded bit width are different wire fields.
	binary.LittleEndian.PutUint32(raw[132:136], 5)
	c := openColumn(t, raw)
	got, err := c.Values(context.Background(), []byte{0x1d, 0, 0, 0}, 0, 0, 1)
	if err != nil || len(got) != 1 || int64(got[0]) != -3 {
		t.Fatalf("%v %v", got, err)
	}
}

func TestIdenticalStorageDescriptorsKeepColumnIdentity(t *testing.T) {
	base := columnFixture(2, 0, 0, [3]uint32{11}, words(7, 111))
	raw := make([]byte, 200)
	copy(raw[:112], base[:112])
	binary.LittleEndian.PutUint32(raw[8:12], 2)
	binary.LittleEndian.PutUint32(raw[44:48], 2)
	binary.LittleEndian.PutUint32(raw[56:60], 48)
	binary.LittleEndian.PutUint32(raw[60:64], 16)
	binary.LittleEndian.PutUint32(raw[80:84], 184)
	copy(raw[120:144], base[116:140])
	copy(raw[144:168], base[116:140])
	copy(raw[168:184], words(7, 111, 7, 222))
	c := openColumn(t, raw)
	for column, want := range []uint64{111, 222} {
		got, err := c.Values(context.Background(), nil, 7, column, 1)
		if err != nil || len(got) != 1 || got[0] != want {
			t.Fatalf("column %d: %v %v", column, got, err)
		}
	}
}

func TestStorageRejectsDamageRatherThanReturningZero(t *testing.T) {
	for _, codec := range []uint32{3, 4} {
		parameters := [3]uint32{}
		count := uint32(1)
		if codec == 4 {
			parameters[2] = 2
			count = 2
		}
		c := openColumn(t, columnFixture(codec, 0, 2, parameters, words(1, 2)))
		if got, err := c.Values(context.Background(), []byte{3}, 0, 0, count); !errors.Is(err, table.ErrFormat) || got != nil {
			t.Fatalf("%v %v", got, err)
		}
	}
	raw := columnFixture(2, 0, 0, [3]uint32{}, words(7, 1, 7, 2))
	if _, err := table.OpenColumns(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits); !errors.Is(err, table.ErrFormat) {
		t.Fatal("duplicate common ID accepted")
	}
	c := openColumn(t, columnFixture(1, 7, 64, [3]uint32{}, nil))
	if got, err := c.Values(context.Background(), make([]byte, 8), 0, 0, 1); err == nil || got != nil {
		t.Fatal("short 9-byte span accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := c.Values(ctx, nil, 0, 0, 1); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	for _, column := range []int{-1, 1} {
		if _, err := c.Values(context.Background(), nil, 0, column, 1); !errors.Is(err, table.ErrFormat) {
			t.Fatal(err)
		}
	}
}

func FuzzColumnStorage(f *testing.F) {
	f.Add([]byte{0xff, 0x00}, uint16(3), uint16(9), uint32(1))
	f.Fuzz(func(t *testing.T, row []byte, offset, width uint16, codec uint32) {
		if len(row) > 1024 || codec > 5 {
			t.Skip()
		}
		raw := columnFixture(codec, offset, width, [3]uint32{0, 0, 1}, nil)
		c, err := table.OpenColumns(context.Background(), bytes.NewReader(raw), int64(len(raw)), limits)
		if err != nil {
			return
		}
		_, _ = c.Values(context.Background(), row, 0, 0, 1)
	})
}
