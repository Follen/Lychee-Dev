package bridge

import (
	"encoding/json"
	"testing"
)

func TestSignalsMatchExactIdentityAndFreshSequence(t *testing.T) {
	s := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0", Kind: "loaded", SessionNonce: "new-session", RequestID: "request-1", Character: "character", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 9, InputReady: true, CodeBytes: 8, CodeAdler32: "12345678"}
	data, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := ParseSignal(data)
	if err != nil {
		t.Fatal(err)
	}
	expected := SignalExpectation{Release: s.Release, Kind: s.Kind, SessionNonce: s.SessionNonce, RequestID: s.RequestID, Character: s.Character, Realm: s.Realm, Product: s.Product, Build: s.Build, AfterSequence: 8, RequireInputReady: true}
	if err := parsed.Match(expected); err != nil {
		t.Fatal(err)
	}
	expected.AfterSequence = 9
	if err := parsed.Match(expected); err == nil {
		t.Fatal("stale signal accepted")
	}
	expected.AfterSequence = 8
	expected.SessionNonce = "old-session"
	if err := parsed.Match(expected); err == nil {
		t.Fatal("foreign session accepted")
	}
	expected.SessionNonce = s.SessionNonce
	parsed.InputReady = false
	if err := parsed.Match(expected); err == nil {
		t.Fatal("unsafe input accepted")
	}
}

func TestSignalSequenceMustFitLuaIntegerRange(t *testing.T) {
	s := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "ready", SessionNonce: "session", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.12345", Sequence: 9007199254740991}
	raw, _ := json.Marshal(s)
	if _, err := ParseSignal(raw); err != nil {
		t.Fatal("safe upper bound rejected", err)
	}
	s.Sequence++
	raw, _ = json.Marshal(s)
	if _, err := ParseSignal(raw); err == nil {
		t.Fatal("unsafe sequence decoded")
	}
	if err := s.Match(SignalExpectation{}); err == nil {
		t.Fatal("unsafe typed sequence matched")
	}
}
