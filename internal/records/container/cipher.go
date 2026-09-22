// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from wowdata's BLTE/Salsa20 handling; see THIRD_PARTY_NOTICES.md.
package container

import (
	"context"
	"encoding/binary"
	"fmt"
	"math/bits"
)

func (d *decoder) unlock(raw []byte, index int) ([]byte, error) {
	if len(raw) < 10 || raw[0] != 8 {
		return nil, ErrMalformed
	}
	name := binary.LittleEndian.Uint64(raw[1:9])
	ivBytes := int(raw[9])
	if (ivBytes != 4 && ivBytes != 8) || len(raw) < 11+ivBytes+1 {
		return nil, ErrMalformed
	}
	if raw[10+ivBytes] != 'S' {
		return nil, ErrUnsupported
	}
	if d.keys == nil {
		return nil, fmt.Errorf("%w: %016x", ErrKeyUnavailable, name)
	}
	key, err := d.keys(d.ctx, name)
	if cancel := d.ctx.Err(); cancel != nil {
		return nil, cancel
	}
	if err != nil {
		return nil, fmt.Errorf("%w: %016x", ErrKeyUnavailable, name)
	}
	if len(key) != 16 && len(key) != 32 {
		return nil, fmt.Errorf("%w: invalid key length for %016x", ErrKeyUnavailable, name)
	}
	var nonce [8]byte
	copy(nonce[:], raw[10:10+ivBytes])
	for n := 0; n < 4; n++ {
		nonce[n] ^= byte(index >> (8 * n))
	}
	return salsaPayload(d.ctx, raw[11+ivBytes:], key, nonce)
}

// salsaPayload only implements the Salsa20/20 form required by BLTE. It keeps
// a single 64-byte keystream block, rather than allocating a second full payload.
func salsaPayload(ctx context.Context, input, key []byte, nonce [8]byte) ([]byte, error) {
	var state [16]uint32
	constant := "expand 32-byte k"
	if len(key) == 16 {
		constant = "expand 16-byte k"
	}
	for i, position := range []int{0, 5, 10, 15} {
		state[position] = binary.LittleEndian.Uint32([]byte(constant[i*4 : i*4+4]))
	}
	for i := 0; i < 4; i++ {
		state[1+i] = binary.LittleEndian.Uint32(key[i*4 : i*4+4])
		offset := (16 + i*4) % len(key)
		state[11+i] = binary.LittleEndian.Uint32(key[offset : offset+4])
	}
	state[6], state[7] = binary.LittleEndian.Uint32(nonce[:4]), binary.LittleEndian.Uint32(nonce[4:])
	output := make([]byte, len(input))
	for start := 0; start < len(input); start += 64 {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		work := state
		for round := 0; round < 10; round++ {
			quarter(&work, 0, 4, 8, 12)
			quarter(&work, 5, 9, 13, 1)
			quarter(&work, 10, 14, 2, 6)
			quarter(&work, 15, 3, 7, 11)
			quarter(&work, 0, 1, 2, 3)
			quarter(&work, 5, 6, 7, 4)
			quarter(&work, 10, 11, 8, 9)
			quarter(&work, 15, 12, 13, 14)
		}
		var stream [64]byte
		for i := range work {
			binary.LittleEndian.PutUint32(stream[i*4:], work[i]+state[i])
		}
		for i := 0; i < min(64, len(input)-start); i++ {
			output[start+i] = input[start+i] ^ stream[i]
		}
		state[8]++
		if state[8] == 0 {
			state[9]++
		}
	}
	return output, nil
}

func quarter(x *[16]uint32, a, b, c, d int) {
	x[b] ^= bits.RotateLeft32(x[a]+x[d], 7)
	x[c] ^= bits.RotateLeft32(x[b]+x[a], 9)
	x[d] ^= bits.RotateLeft32(x[c]+x[b], 13)
	x[a] ^= bits.RotateLeft32(x[d]+x[c], 18)
}
