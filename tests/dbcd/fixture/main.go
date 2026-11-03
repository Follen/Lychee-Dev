// Command fixture creates independent synthetic WDC3/DBD inputs for the pinned
// oracle. The file has external IDs, Common overrides, and one copied row.
package main

import (
	"encoding/binary"
	"flag"
	"fmt"
	"math"
	"os"
	"path/filepath"
)

func main() {
	out := flag.String("out", "", "fixture directory")
	flag.Parse()
	if *out == "" {
		panic("-out required")
	}
	if err := os.MkdirAll(*out, 0700); err != nil {
		panic(err)
	}
	numericFixture(*out)
	for _, tc := range []struct {
		name      string
		overrides []uint32
		n         int
	}{
		{"CommonBoth", []uint32{7, 99, 900, 77}, 1},
		{"CommonOrigin", []uint32{7, 99}, 1},
		{"CommonDefault", nil, 1},
		{"FullFields", []uint32{207, 99, 900, 77}, 300},
	} {
		aux := len(tc.overrides) * 4
		raw := make([]byte, 140+aux+tc.n*4+tc.n*4+8)
		copy(raw, "WDC3")
		put := func(at int, v uint32) { binary.LittleEndian.PutUint32(raw[at:], v) }
		put(4, uint32(tc.n))
		put(8, 1)
		put(12, 4)
		put(20, 0xaabbccdd)
		put(24, 0x12345678)
		put(28, 7)
		put(32, 900)
		binary.LittleEndian.PutUint16(raw[40:], 4)
		put(44, 1)
		put(56, 24)
		put(60, uint32(aux))
		put(68, 1)
		put(80, uint32(140+aux))
		put(84, uint32(tc.n))
		put(96, uint32(tc.n*4))
		put(108, 1)
		put(120, uint32(aux))
		put(124, 2)
		put(128, 33)
		for i, v := range tc.overrides {
			put(140+i*4, v)
		}
		idsAt := 140 + aux + tc.n*4
		for i := 0; i < tc.n; i++ {
			put(idsAt+i*4, uint32(i+7))
		}
		put(len(raw)-8, 900)
		put(len(raw)-4, 7)
		if err := os.WriteFile(filepath.Join(*out, tc.name+".db2"), raw, 0600); err != nil {
			panic(err)
		}
		dbd := "COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$noninline,id$ID<32>\nValue<u32>\n"
		if err := os.WriteFile(filepath.Join(*out, tc.name+".dbd"), []byte(dbd), 0600); err != nil {
			panic(err)
		}
		fmt.Println(tc.name)
	}
}
func numericFixture(out string) {
	const rows, fields, size = 301, 3, 20
	const start = 112 + fields*28
	raw := make([]byte, start+rows*size+rows*4)
	copy(raw, "WDC3")
	put := func(at int, v uint32) { binary.LittleEndian.PutUint32(raw[at:], v) }
	put(4, rows)
	put(8, fields)
	put(12, size)
	put(20, 0xaabbccdd)
	put(24, 0x12345678)
	put(28, 7)
	put(32, 307)
	binary.LittleEndian.PutUint16(raw[40:], 4)
	put(44, fields)
	put(56, fields*24)
	put(68, 1)
	put(80, start)
	put(84, rows)
	put(96, rows*4)
	for i, f := range []struct{ offset, width uint16 }{{0, 64}, {8, 32}, {12, 64}} {
		binary.LittleEndian.PutUint16(raw[112+i*4:], uint16(int16(32-int(f.width))))
		binary.LittleEndian.PutUint16(raw[114+i*4:], f.offset)
		binary.LittleEndian.PutUint16(raw[124+i*24:], f.offset*8)
		binary.LittleEndian.PutUint16(raw[126+i*24:], f.width)
	}
	for i := 0; i < rows; i++ {
		at := start + i*size
		binary.LittleEndian.PutUint64(raw[at:], ^uint64(0)-uint64(i))
		put(at+8, math.Float32bits(float32(i)+0.1))
		binary.LittleEndian.PutUint64(raw[at+12:], uint64(-int64(i)-1))
		put(start+rows*size+i*4, uint32(i+7))
	}
	dbd := "COLUMNS\nint ID\nint Large\nfloat Fraction\nint Signed\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$noninline,id$ID<32>\nLarge<u64>\nFraction\nSigned<64>\n"
	if err := os.WriteFile(filepath.Join(out, "Numeric.db2"), raw, 0600); err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(out, "Numeric.dbd"), []byte(dbd), 0600); err != nil {
		panic(err)
	}
	fmt.Println("Numeric")
}
