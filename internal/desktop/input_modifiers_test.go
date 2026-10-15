package desktop

import (
	"errors"
	"testing"
	"time"
)

func TestBackgroundChordRejectsHeldKeyboardKeys(t *testing.T) {
	for _, held := range []uint16{0, 0x03, 0x08, 0x10, 0x11, 0x12, 0x5B, 0x5C, 0x7B, 0xA5, 0xBA, 'A', 'W', 0x20} {
		err := checkKeyboardKeys(func(vk uint16) bool { return held == vk })
		wantBlocked := held != 0
		if errors.Is(err, ErrKeyAlreadyHeld) != wantBlocked {
			t.Fatalf("held=%#x: %v", held, err)
		}
	}
}

func TestKeyboardReadinessWaitsForReleaseAndHalfSecondQuiet(t *testing.T) {
	start := time.Now()
	state := &keyboardActivityState{lastEvent: start}
	for _, step := range []struct {
		name  string
		at    time.Duration
		event bool
		held  uint16
		want  error
	}{
		{"startup", 499 * time.Millisecond, false, 0, ErrKeyboardActive},
		{"startup_quiet", 500 * time.Millisecond, false, 0, nil},
		{"w_down", time.Second, true, 'W', ErrKeyAlreadyHeld},
		{"w_held_without_repeats", 10 * time.Second, false, 'W', ErrKeyAlreadyHeld},
		{"w_release", 11 * time.Second, true, 0, ErrKeyboardActive},
		{"release_499ms", 11499 * time.Millisecond, false, 0, ErrKeyboardActive},
		{"brief_new_key_down", 11500 * time.Millisecond, true, 'A', ErrKeyAlreadyHeld},
		{"brief_key_up", 11510 * time.Millisecond, true, 0, ErrKeyboardActive},
		{"release_restarts_quiet", 12009 * time.Millisecond, false, 0, ErrKeyboardActive},
		{"all_released_500ms", 12010 * time.Millisecond, false, 0, nil},
	} {
		now := start.Add(step.at)
		if step.event {
			state.note(now)
		}
		err := state.check(now, func(vk uint16) bool { return vk == step.held })
		if !errors.Is(err, step.want) {
			t.Fatalf("%s: %v want %v", step.name, err, step.want)
		}
	}
	for _, mouse := range []uint16{1, 2, 4, 5, 6} {
		if err := state.check(start.Add(20*time.Second), func(vk uint16) bool { return vk == mouse }); err != nil {
			t.Fatalf("mouse button counted as keyboard activity: %#x %v", mouse, err)
		}
	}
}
