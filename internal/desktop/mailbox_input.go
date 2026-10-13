package desktop

import (
	"errors"
	"strings"
)

// MailboxBindings are the two formal actions declared by the native addon.
type MailboxBindings struct {
	Wake   string `json:"wake"`
	Reload string `json:"reload"`
}

func ValidateMailboxBindings(b MailboxBindings) error {
	for _, chord := range []string{b.Wake, b.Reload} {
		key, err := receiverBindingKey(chord)
		if err != nil || !strings.HasPrefix(key, "F") || strings.Contains(chord, "SHIFT") {
			return errors.New("desktop.mailbox_binding_invalid")
		}
	}
	if b.Wake == b.Reload {
		return errors.New("desktop.mailbox_binding_duplicate")
	}
	return nil
}
