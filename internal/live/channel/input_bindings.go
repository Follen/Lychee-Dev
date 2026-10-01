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
	Submit string `json:"submit"`
	Close  string `json:"close"`
}

func (b *InputBindings) UnmarshalJSON(data []byte) error {
	var wire struct {
		Wake   *string `json:"wake"`
		Submit *string `json:"submit"`
		Close  *string `json:"close"`
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if d.Decode(&wire) != nil || d.Decode(new(any)) != io.EOF || wire.Wake == nil || wire.Submit == nil || wire.Close == nil {
		return errors.New("live.channel_input_bindings_invalid")
	}
	*b = InputBindings{*wire.Wake, *wire.Submit, *wire.Close}
	return nil
}

func primaryInputBindings() *InputBindings {
	return &InputBindings{"ALT-CTRL-F12", "ALT-CTRL-SHIFT-F12", "ALT-CTRL-["}
}
func fallbackInputBindings() *InputBindings {
	return &InputBindings{"ALT-CTRL-F11", "ALT-CTRL-SHIFT-F11", "ALT-CTRL-["}
}
func (s InputObservation) validBindings() bool {
	if s.Bindings == nil || s.InputBlocked == nil {
		return false
	}
	b := *s.Bindings
	if b == (InputBindings{}) {
		return *s.InputBlocked && s.Reason == "input_binding_unavailable"
	}
	return s.Reason != "input_binding_unavailable" && (b == *primaryInputBindings() || b == *fallbackInputBindings())
}

// receiverProfile grants key authority only to a complete effective profile.
func (s InputObservation) receiverProfile() (desktop.ReceiverBindings, error) {
	if s.Schema != InputSchema || !s.validBindings() || *s.Bindings == (InputBindings{}) {
		return desktop.ReceiverBindings{}, errors.New("live.channel_input_bindings_invalid")
	}
	return desktop.ReceiverBindings{WakeBinding: s.Bindings.Wake, SubmitBinding: s.Bindings.Submit, CloseBinding: s.Bindings.Close}, nil
}

// Equal timestamps alone cannot authorize a changed publication's key profile.
func sameInputProfile(current, stored InputObservation) bool {
	return current.validBindings() && stored.validBindings() && current.Bindings != nil && stored.Bindings != nil && *current.Bindings == *stored.Bindings
}
