package desktop

import "errors"

// InputReceipt reports queue submission only, never game execution.
type InputReceipt struct {
	MessagesQueued     int  `json:"messagesQueued"`
	SubmissionComplete bool `json:"submissionComplete"`
}

var ErrKeyAlreadyHeld = errors.New("desktop.key_already_held")

func checkChordKeys(keys []uint16, pressed func(uint16) bool) error {
	for _, vk := range []uint16{0x10, 0x11, 0x12, 0x5B, 0x5C} {
		if pressed(vk) {
			return ErrKeyAlreadyHeld
		}
	}
	for _, vk := range keys {
		if pressed(vk) {
			return ErrKeyAlreadyHeld
		}
	}
	return nil
}
