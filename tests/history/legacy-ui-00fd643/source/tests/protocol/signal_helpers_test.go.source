package protocol_test

import (
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// parseSessionSignal decodes one receipt exactly the way the live reader does:
// the wire omits the actor identity to save QR modules, and the host fills it
// back from the baseline its retained session already proved before matching.
// A test that asserts on character, realm or guid must go through this helper,
// because ParseSignal alone deliberately leaves those fields empty.
func parseSessionSignal(t *testing.T, raw []byte, baseline bridge.SignalIdentity) bridge.Signal {
	t.Helper()
	signal, err := bridge.ParseSignal(raw)
	if err != nil {
		t.Fatalf("ParseSignal(%q): %v", raw, err)
	}
	return bridge.FillSignalIdentity(signal, baseline)
}

// sessionBaseline is the identity a retained fixture session proved: the same
// values the Lua fixtures feed ObserveActor, plus the session nonce the host
// bound. The compact wire form omits all of them.
func sessionBaseline(product, build string) bridge.SignalIdentity {
	return bridge.SignalIdentity{
		Release:      "2.0.2",
		Character:    "Paladin",
		Realm:        "Realm",
		GUID:         "Player-1-123",
		Product:      product,
		Build:        build,
		SessionNonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	}
}
