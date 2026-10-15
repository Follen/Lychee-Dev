package channel

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"

	"github.com/follenfang/lycheedev/internal/desktop"
)

// InputBindings is the complete effective private profile published by INPUT.
// A missing member is distinct from an explicitly unavailable empty profile.
type InputBindings struct {
	Wake   string `json:"wake"`
	Reload string `json:"reload,omitempty"`
	// Retained only to read existing journals. These fields grant no input.
	Submit string `json:"submit,omitempty"`
	Close  string `json:"close,omitempty"`
}

func (b InputBindings) MarshalJSON() ([]byte, error) {
	if b.Submit == "" && b.Close == "" {
		return json.Marshal(desktop.MailboxBindings{Wake: b.Wake, Reload: b.Reload})
	}
	type legacy struct {
		Wake   string `json:"wake"`
		Submit string `json:"submit"`
		Close  string `json:"close"`
	}
	return json.Marshal(legacy{b.Wake, b.Submit, b.Close})
}

func (b *InputBindings) UnmarshalJSON(data []byte) error {
	var wire struct {
		Wake   *string `json:"wake"`
		Reload *string `json:"reload"`
		Submit *string `json:"submit"`
		Close  *string `json:"close"`
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if d.Decode(&wire) != nil || d.Decode(new(any)) != io.EOF || wire.Wake == nil {
		return errors.New("live.channel_input_bindings_invalid")
	}
	if wire.Reload != nil && wire.Submit == nil && wire.Close == nil {
		*b = InputBindings{Wake: *wire.Wake, Reload: *wire.Reload}
	} else if wire.Reload == nil && wire.Submit != nil && wire.Close != nil {
		*b = InputBindings{Wake: *wire.Wake, Submit: *wire.Submit, Close: *wire.Close}
	} else {
		return errors.New("live.channel_input_bindings_invalid")
	}
	return nil
}

func primaryInputBindings() *InputBindings {
	return &InputBindings{Wake: "ALT-CTRL-F12", Reload: "ALT-CTRL-F11"}
}
func fallbackInputBindings() *InputBindings {
	return &InputBindings{Wake: "ALT-CTRL-F10", Reload: "ALT-CTRL-F9"}
}
func (s InputObservation) validBindings() bool {
	if s.Bindings == nil || s.InputBlocked == nil {
		return false
	}
	b := *s.Bindings
	if b == (InputBindings{}) {
		return *s.InputBlocked && s.Reason == "input_binding_unavailable"
	}
	if s.Reason == "input_binding_unavailable" {
		return false
	}
	if b.Reload != "" {
		return b.Submit == "" && b.Close == "" && desktop.ValidateMailboxBindings(desktop.MailboxBindings{Wake: b.Wake, Reload: b.Reload}) == nil
	}
	return false
}

// mailboxProfile grants key authority only to a complete effective profile.
func (s InputObservation) mailboxProfile(action string) (desktop.MailboxBindings, error) {
	if !s.validSchema(action) || !s.validBindings() || *s.Bindings == (InputBindings{}) {
		return desktop.MailboxBindings{}, errors.New("live.channel_input_bindings_invalid")
	}
	return desktop.MailboxBindings{Wake: s.Bindings.Wake, Reload: s.Bindings.Reload}, nil
}

// Equal timestamps alone cannot authorize a changed publication's key profile.
func sameInputProfile(current, stored InputObservation) bool {
	return current.validBindings() && stored.validBindings() && current.Bindings != nil && stored.Bindings != nil && *current.Bindings == *stored.Bindings && current.SlotReloadRequired == stored.SlotReloadRequired
}
