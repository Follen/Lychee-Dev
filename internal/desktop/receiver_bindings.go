package desktop

import (
	"errors"
	"fmt"
	"strings"
)

// ReceiverBindings names the three chords understood by the in-game receiver.
// Only the finite set supported by both the addon and Windows sender is legal.
type ReceiverBindings struct {
	WakeBinding   string `json:"wakeBinding"`
	SubmitBinding string `json:"submitBinding"`
	CloseBinding  string `json:"closeBinding"`
}

func DefaultReceiverBindings() ReceiverBindings {
	return ReceiverBindings{WakeBinding: "ALT-CTRL-]", SubmitBinding: "ALT-CTRL-SHIFT-]", CloseBinding: "ALT-CTRL-["}
}

func receiverBindingKey(chord string) (string, error) {
	key := ""
	switch {
	case strings.HasPrefix(chord, "ALT-CTRL-SHIFT-"):
		key = strings.TrimPrefix(chord, "ALT-CTRL-SHIFT-")
	case strings.HasPrefix(chord, "ALT-CTRL-"):
		key = strings.TrimPrefix(chord, "ALT-CTRL-")
	default:
		return "", errors.New("desktop.receiver_binding_invalid")
	}
	if key == "[" || key == "]" {
		return key, nil
	}
	if len(key) >= 2 && len(key) <= 3 && key[0] == 'F' {
		var n int
		if _, err := fmt.Sscanf(key, "F%d", &n); err == nil && n >= 1 && n <= 12 && fmt.Sprintf("F%d", n) == key {
			return key, nil
		}
	}
	return "", errors.New("desktop.receiver_binding_invalid")
}

func ValidateReceiverBindings(bindings ReceiverBindings) error {
	chords := []string{bindings.WakeBinding, bindings.SubmitBinding, bindings.CloseBinding}
	seen := map[string]bool{}
	keys := make([]string, 0, 3)
	for _, chord := range chords {
		key, err := receiverBindingKey(chord)
		if err != nil {
			return err
		}
		keys = append(keys, key)
		if seen[chord] {
			return errors.New("desktop.receiver_binding_duplicate")
		}
		seen[chord] = true
	}
	// The focused EditBox observes the terminal key, while modifier state is not
	// reliable in the proven background PostMessage path. Close must remain
	// distinguishable from both other receiver actions on that path.
	if keys[2] == keys[0] || keys[2] == keys[1] {
		return errors.New("desktop.receiver_close_key_conflict")
	}
	return nil
}
