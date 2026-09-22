package bridge

import (
	"encoding/json"
	"strings"
	"testing"
)

func identitySignal() Signal {
	return Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "identity",
		ProbeNonce: strings.Repeat("0123456789abcdef", 2), ActorState: "ok",
		Character: "Paladin", Realm: "Realm", GUID: "Player-1-123",
		Product: "retail", Build: "12.1.0.69875", Sequence: 0, InputReady: true}
}

func encoded(t *testing.T, s Signal) []byte {
	t.Helper()
	data, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func TestIdentitySignalParsesOnlyAsAnIdentityMarker(t *testing.T) {
	for _, mode := range []string{"ok", "no-actor", "actor-restricted", "runtime-epoch", "not-ready-reason"} {
		t.Run(mode, func(t *testing.T) {
			s := identitySignal()
			switch mode {
			case "no-actor":
				s.ActorState, s.Character, s.Realm, s.GUID, s.InputReady = "no_actor", "", "", "", false
				s.InputReason = "input_not_logged_in"
			case "actor-restricted":
				s.ActorState, s.Character, s.Realm, s.GUID = "actor_restricted", "", "", ""
			case "runtime-epoch":
				s.RuntimeEpoch = 7
			case "not-ready-reason":
				s.InputReady, s.InputReason = false, "input_keyboard_focus"
			}
			parsed, err := ParseSignal(encoded(t, s))
			if err != nil {
				t.Fatal(err)
			}
			if parsed != s {
				t.Fatalf("round trip changed identity signal: %+v", parsed)
			}
			expected := SignalExpectation{Kind: "identity", Release: s.Release, ProbeNonce: s.ProbeNonce, ActorState: s.ActorState, Product: s.Product, Build: s.Build}
			if err := parsed.Match(expected); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestIdentitySignalRejectsProtocolCarryOver(t *testing.T) {
	for _, mode := range []string{"session", "request", "reload-nonce", "cleanup-nonce", "sequence", "code", "report", "checksum", "probe-on-ready", "actor-state-on-ready", "reason-on-ready"} {
		t.Run(mode, func(t *testing.T) {
			s := identitySignal()
			switch mode {
			case "session":
				s.SessionNonce = strings.Repeat("a", 32)
			case "request":
				s.RequestID = "REQ-one"
			case "reload-nonce":
				s.ReloadNonce = strings.Repeat("b", 32)
			case "cleanup-nonce":
				s.CleanupNonce = strings.Repeat("c", 32)
			case "sequence":
				s.Sequence = 1
			case "code":
				s.CodeBytes, s.CodeAdler32 = 4, "00000001"
			case "report":
				s.ReportBytes, s.ReportAdler32 = 4, "00000001"
			case "checksum":
				s.CodeAdler32 = "00000001"
			case "probe-on-ready":
				s.Kind, s.SessionNonce, s.Sequence = "ready", strings.Repeat("a", 32), 1
			case "actor-state-on-ready":
				s.Kind, s.SessionNonce, s.Sequence = "ready", strings.Repeat("a", 32), 1
				s.ProbeNonce, s.ActorState = "", "ok"
				s.Character, s.Realm = "Paladin", "Realm"
			case "reason-on-ready":
				s.Kind, s.SessionNonce, s.Sequence = "ready", strings.Repeat("a", 32), 1
				s.ProbeNonce, s.ActorState, s.InputReason = "", "", "input_keyboard_focus"
				s.Character, s.Realm = "Paladin", "Realm"
			}
			if _, err := ParseSignal(encoded(t, s)); err == nil {
				t.Fatalf("accepted %s", mode)
			}
		})
	}
}

func TestIdentitySignalProbeNonceAndActorStateAreExact(t *testing.T) {
	for _, mode := range []string{"missing", "short", "long", "uppercase", "non-hex", "actor-missing", "actor-invalid", "empty-actor-ok", "empty-realm-ok", "actor-present-not-ok", "guid-present-not-ok", "reason-ready", "reason-invalid"} {
		t.Run(mode, func(t *testing.T) {
			s := identitySignal()
			switch mode {
			case "missing":
				s.ProbeNonce = ""
			case "short":
				s.ProbeNonce = s.ProbeNonce[:31]
			case "long":
				s.ProbeNonce += "0"
			case "uppercase":
				s.ProbeNonce = strings.ToUpper(s.ProbeNonce)
			case "non-hex":
				s.ProbeNonce = strings.Repeat("z", 32)
			case "actor-missing":
				s.ActorState = ""
			case "actor-invalid":
				s.ActorState = "loading"
			case "empty-actor-ok":
				s.Character = ""
			case "empty-realm-ok":
				s.Realm = ""
			case "actor-present-not-ok":
				s.ActorState = "no_actor"
			case "guid-present-not-ok":
				s.ActorState, s.Character, s.Realm = "no_actor", "", ""
			case "reason-ready":
				s.InputReason = "input_keyboard_focus"
			case "reason-invalid":
				s.InputReady, s.InputReason = false, "KeyboardFocus"
			}
			if _, err := ParseSignal(encoded(t, s)); err == nil {
				t.Fatalf("accepted %s", mode)
			}
		})
	}
}

func TestIdentityExpectationsNeverCrossKinds(t *testing.T) {
	identity := identitySignal()
	ready := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "ready", SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.69875", Sequence: 1, InputReady: true}
	parsedIdentity, err := ParseSignal(encoded(t, identity))
	if err != nil {
		t.Fatal(err)
	}
	parsedReady, err := ParseSignal(encoded(t, ready))
	if err != nil {
		t.Fatal(err)
	}
	identityExpectation := SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, Product: identity.Product, Build: identity.Build}
	readyExpectation := SignalExpectation{Kind: "ready", Release: ready.Release, SessionNonce: ready.SessionNonce, Character: ready.Character, Realm: ready.Realm, Product: ready.Product, Build: ready.Build, AfterSequence: 0}
	if err := parsedIdentity.Match(identityExpectation); err != nil {
		t.Fatal(err)
	}
	if err := parsedReady.Match(readyExpectation); err != nil {
		t.Fatal(err)
	}
	for _, rejected := range []struct {
		name       string
		signal     Signal
		expectation SignalExpectation
	}{
		{"ready-vs-identity", parsedReady, identityExpectation},
		{"identity-vs-ready", parsedIdentity, readyExpectation},
		{"identity-vs-empty-kind", parsedIdentity, SignalExpectation{Release: identity.Release}},
		{"wrong-probe", parsedIdentity, SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: strings.Repeat("f", 32), Product: identity.Product, Build: identity.Build}},
		{"missing-probe-expectation", parsedIdentity, SignalExpectation{Kind: "identity", Release: identity.Release, Product: identity.Product, Build: identity.Build}},
		{"wrong-actor-state", parsedIdentity, SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, ActorState: "no_actor", Product: identity.Product, Build: identity.Build}},
		{"wrong-build", parsedIdentity, SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, Product: identity.Product, Build: "12.1.0.69876"}},
		{"stale-sequence-watermark", parsedIdentity, SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, Product: identity.Product, Build: identity.Build, AfterSequence: 1}},
	} {
		if err := rejected.signal.Match(rejected.expectation); err == nil {
			t.Fatalf("accepted %s", rejected.name)
		}
	}
	unready := identity
	unready.InputReady, unready.InputReason = false, "input_combat_lockdown"
	parsedUnready, err := ParseSignal(encoded(t, unready))
	if err != nil {
		t.Fatal(err)
	}
	if err := parsedUnready.Match(SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, Product: identity.Product, Build: identity.Build}); err != nil {
		t.Fatal(err)
	}
	if err := parsedUnready.Match(SignalExpectation{Kind: "identity", Release: identity.Release, ProbeNonce: identity.ProbeNonce, Product: identity.Product, Build: identity.Build, RequireInputReady: true}); err == nil {
		t.Fatal("input-ready gate matched an unready identity receipt")
	}
}
