// Jenkins lookup3 was placed in the public domain by Bob Jenkins (May 2006).
// Algorithm reference: https://burtleburtle.net/bob/c/lookup3.c
package archive

import (
	"encoding/binary"
	"math/bits"
)

// guardSum computes the two-word little-endian lookup3 format checksum.
// It detects accidental corruption, not adversarial substitution.
func guardSum(data []byte, primary, secondary uint32) (uint32, uint32) {
	a := uint32(0xdeadbeef) + uint32(len(data)) + primary
	b, c := a, a+secondary
	for len(data) > 12 {
		a += binary.LittleEndian.Uint32(data)
		b += binary.LittleEndian.Uint32(data[4:])
		c += binary.LittleEndian.Uint32(data[8:])
		a -= c
		a ^= bits.RotateLeft32(c, 4)
		c += b
		b -= a
		b ^= bits.RotateLeft32(a, 6)
		a += c
		c -= b
		c ^= bits.RotateLeft32(b, 8)
		b += a
		a -= c
		a ^= bits.RotateLeft32(c, 16)
		c += b
		b -= a
		b ^= bits.RotateLeft32(a, 19)
		a += c
		c -= b
		c ^= bits.RotateLeft32(b, 4)
		b += a
		data = data[12:]
	}
	if len(data) == 0 {
		return c, b
	}
	for i, v := range data {
		switch {
		case i < 4:
			a += uint32(v) << (8 * i)
		case i < 8:
			b += uint32(v) << (8 * (i - 4))
		default:
			c += uint32(v) << (8 * (i - 8))
		}
	}
	c ^= b
	c -= bits.RotateLeft32(b, 14)
	a ^= c
	a -= bits.RotateLeft32(c, 11)
	b ^= a
	b -= bits.RotateLeft32(a, 25)
	c ^= b
	c -= bits.RotateLeft32(b, 16)
	a ^= c
	a -= bits.RotateLeft32(c, 4)
	b ^= a
	b -= bits.RotateLeft32(a, 14)
	c ^= b
	c -= bits.RotateLeft32(b, 24)
	return c, b
}
