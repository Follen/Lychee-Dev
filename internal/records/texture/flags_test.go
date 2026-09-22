package texture_test

import (
	"context"
	"encoding/binary"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/texture"
)

func TestReadTextureExtendedMipFlags(t *testing.T) {
	// Retail file 134400 uses 0x11 in this byte. It is not a boolean;
	// mip availability and bounds come from the offset/size tables.
	for _, flags := range []byte{1, 0x11} {
		raw := make([]byte, 156)
		copy(raw, "BLP2")
		binary.LittleEndian.PutUint32(raw[4:], 1)
		raw[8], raw[9], raw[11] = 2, 1, flags
		binary.LittleEndian.PutUint32(raw[12:], 1)
		binary.LittleEndian.PutUint32(raw[16:], 1)
		binary.LittleEndian.PutUint32(raw[20:], 148)
		binary.LittleEndian.PutUint32(raw[84:], 8)
		binary.LittleEndian.PutUint16(raw[148:], 0x07e0)
		pixels, _, err := texture.ReadTexture(context.Background(), raw, 0, 1)
		if err != nil {
			t.Errorf("flags=%#x: %v", flags, err)
			continue
		}
		if got := pixels.NRGBAAt(0, 0); got.G != 255 || got.A != 255 || got.R != 0 || got.B != 0 {
			t.Errorf("flags=%#x: pixel=%v", flags, got)
		}
	}
}
