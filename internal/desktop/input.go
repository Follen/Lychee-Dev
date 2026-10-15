package desktop

import (
	"errors"
	"sync"
	"time"
)

// InputReceipt reports queue submission only, never game execution.
type InputReceipt struct {
	MessagesQueued     int  `json:"messagesQueued"`
	SubmissionComplete bool `json:"submissionComplete"`
}

var ErrKeyAlreadyHeld = errors.New("desktop.key_already_held")
var ErrKeyboardActive = errors.New("desktop.keyboard_active")

const keyboardQuietPeriod = 500 * time.Millisecond

func checkKeyboardKeys(pressed func(uint16) bool) error {
	// VK_CANCEL (Ctrl+Break) is a keyboard key; 0x04..0x06 are mouse
	// buttons and 0x07 is reserved. All other keyboard VKs start at 0x08.
	for vk := uint16(0x03); vk < 0xff; vk++ {
		if vk >= 0x04 && vk <= 0x07 {
			continue
		}
		if pressed(vk) {
			return ErrKeyAlreadyHeld
		}
	}
	return nil
}

// keyboardActivityState retains only the time of the latest event, never keys
// or typed text. time.Time preserves the monotonic clock for the quiet period.
type keyboardActivityState struct {
	mu        sync.Mutex
	lastEvent time.Time
	err       error
}

func (s *keyboardActivityState) note(at time.Time) {
	s.mu.Lock()
	s.lastEvent = at
	s.mu.Unlock()
}

func (s *keyboardActivityState) stop(err error) {
	s.mu.Lock()
	s.err = err
	s.mu.Unlock()
}

func (s *keyboardActivityState) check(now time.Time, pressed func(uint16) bool) error {
	held := checkKeyboardKeys(pressed)
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.err != nil {
		return s.err
	}
	if held != nil {
		return held
	}
	if now.Sub(s.lastEvent) < keyboardQuietPeriod {
		return ErrKeyboardActive
	}
	return nil
}
