package bridge

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestClearedSignalRejectsIncompleteOrMixedEvidence(t *testing.T) {
	valid := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "cleared", SessionNonce: strings.Repeat("a", 32), RequestID: "REQ-cleanup", CleanupNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Sequence: 1}
	for name, change := range map[string]func(*Signal){
		"missing-nonce":    func(s *Signal) { s.CleanupNonce = "" },
		"uppercase-nonce":  func(s *Signal) { s.CleanupNonce = strings.Repeat("B", 32) },
		"short-nonce":      func(s *Signal) { s.CleanupNonce = "abc" },
		"missing-guid":     func(s *Signal) { s.GUID = "" },
		"missing-request":  func(s *Signal) { s.RequestID = "" },
		"old-reload":       func(s *Signal) { s.ReloadNonce = strings.Repeat("c", 32) },
		"input-permission": func(s *Signal) { s.InputReady = true },
		"code":             func(s *Signal) { s.CodeBytes = 1; s.CodeAdler32 = "00000001" },
		"body":             func(s *Signal) { s.ReportBytes = 1; s.ReportAdler32 = "00000001" },
		"digest-only":      func(s *Signal) { s.CodeAdler32 = "00000001" },
		"wrong-kind":       func(s *Signal) { s.Kind = "ready" },
	} {
		t.Run(name, func(t *testing.T) {
			s := valid
			change(&s)
			raw, _ := json.Marshal(s)
			if _, err := ParseSignal(raw); err == nil {
				t.Fatal("accepted invalid cleared signal")
			}
		})
	}
	raw, _ := json.Marshal(valid)
	if _, err := ParseSignal(raw); err != nil {
		t.Fatal(err)
	}
}

func TestClearedSignalAcceptsRuntimeEpochBoundaryAndRejectsOverflow(t *testing.T) {
	valid := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "cleared", SessionNonce: strings.Repeat("a", 32), RequestID: "REQ-cleanup", CleanupNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Sequence: 1, RuntimeEpoch: 9007199254740991}
	raw, err := json.Marshal(valid)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ParseSignal(raw); err != nil {
		t.Fatalf("accepted boundary runtime epoch: %v", err)
	}

	valid.RuntimeEpoch++
	raw, err = json.Marshal(valid)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ParseSignal(raw); err == nil {
		t.Fatal("accepted overflowing runtime epoch")
	}
}

func TestOnlyReadyAndClearedSignalsAcceptRuntimeEpoch(t *testing.T) {
	signal := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "loaded", SessionNonce: strings.Repeat("a", 32), RequestID: "REQ-runtime", Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.12345", Sequence: 1, CodeBytes: 1, CodeAdler32: "00000001", RuntimeEpoch: 1}
	raw, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ParseSignal(raw); err == nil {
		t.Fatal("accepted runtime epoch on non-ready/non-cleared signal")
	}
}

func TestClearedSignalMatchRejectsDifferentRuntimeEpoch(t *testing.T) {
	signal := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "cleared", SessionNonce: strings.Repeat("a", 32), RequestID: "REQ-cleanup", CleanupNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Sequence: 2, RuntimeEpoch: 42}
	raw, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := ParseSignal(raw)
	if err != nil {
		t.Fatal(err)
	}
	expected := SignalExpectation{Release: signal.Release, Kind: signal.Kind, SessionNonce: signal.SessionNonce, RequestID: signal.RequestID, CleanupNonce: signal.CleanupNonce, Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build, AfterSequence: 1, RuntimeEpoch: signal.RuntimeEpoch}
	if err := parsed.Match(expected); err != nil {
		t.Fatal(err)
	}

	expected.RuntimeEpoch++
	if err := parsed.Match(expected); err == nil {
		t.Fatal("accepted cleared signal with a different runtime epoch")
	}
}
