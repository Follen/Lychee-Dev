package desktop

import (
	"errors"
	"testing"
)

func TestBackgroundChordRejectsExtraPhysicalModifiers(t *testing.T) {
	for _, held := range []uint16{0, 0x10, 0x11, 0x12, 0x5B, 0x5C, 0x7B, 'A'} {
		err := checkChordKeys([]uint16{0x11, 0x12, 0x7B}, func(vk uint16) bool { return held == vk })
		wantBlocked := held != 0 && held != 'A'
		if errors.Is(err, ErrKeyAlreadyHeld) != wantBlocked {
			t.Fatalf("held=%#x: %v", held, err)
		}
	}
}
