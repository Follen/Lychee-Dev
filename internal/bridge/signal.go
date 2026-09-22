package bridge

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"unicode/utf8"
)

var ErrSignalIdentity = errors.New("bridge.signal_identity_mismatch")

// Signal is an identity/status receipt, never the report body. Request and code
// digests are verified again against the persisted report before acknowledgement.
// Kind "identity" is an identity marker only: it binds no session, carries no
// request, and its probeNonce correlates exactly one trigger with one receipt.
type Signal struct {
	Schema        string `json:"schema"`
	Release       string `json:"release"`
	Kind          string `json:"kind"`
	SessionNonce  string `json:"sessionNonce"`
	RequestID     string `json:"requestId"`
	ReloadNonce   string `json:"reloadNonce,omitempty"`
	CleanupNonce  string `json:"cleanupNonce,omitempty"`
	ProbeNonce    string `json:"probeNonce,omitempty"`
	ActorState    string `json:"actorState,omitempty"`
	InputReason   string `json:"inputReason,omitempty"`
	Character     string `json:"character,omitempty"`
	Realm         string `json:"realm,omitempty"`
	GUID          string `json:"guid,omitempty"`
	Product       string `json:"product"`
	Build         string `json:"build"`
	Sequence      uint64 `json:"sequence"`
	RuntimeEpoch  uint64 `json:"runtimeEpoch,omitempty"`
	InputReady    bool   `json:"inputReady"`
	CodeBytes     uint32 `json:"codeBytes,omitempty"`
	CodeAdler32   string `json:"codeAdler32,omitempty"`
	ReportBytes   uint32 `json:"reportBytes,omitempty"`
	ReportAdler32 string `json:"reportAdler32,omitempty"`
}

type SignalExpectation struct {
	Release, Kind, SessionNonce, RequestID, ReloadNonce, CleanupNonce string
	ProbeNonce, ActorState, Character, Realm, Product, Build          string
	AfterSequence                                                     uint64
	RuntimeEpoch                                                      uint64
	RequireInputReady                                                 bool
}

func ParseSignal(data []byte) (Signal, error) {
	var signal Signal
	if len(data) == 0 || len(data) > 4096 || !utf8.Valid(data) {
		return signal, errors.New("bridge.signal_budget_or_encoding")
	}
	if err := validateJSON(data, 2, 32); err != nil {
		return signal, err
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&signal); err != nil {
		return signal, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return signal, errors.New("bridge.signal_trailing_data")
	}
	if signal.Schema != "lycheedev.signal.v1" || signal.Release == "" || signal.Product == "" || signal.Build == "" {
		return signal, errors.New("bridge.invalid_signal")
	}
	if signal.RuntimeEpoch > 9007199254740991 {
		return signal, errors.New("bridge.invalid_runtime_epoch")
	}
	switch signal.Kind {
	case "identity":
		return parseIdentitySignal(signal)
	case "ready", "loaded", "reported", "acknowledged", "cancelled", "cleared":
	default:
		return signal, errors.New("bridge.invalid_signal_kind")
	}
	if signal.RuntimeEpoch != 0 && signal.Kind != "ready" && signal.Kind != "cleared" {
		return signal, errors.New("bridge.invalid_runtime_epoch")
	}
	return parseSessionSignal(signal)
}

// parseSessionSignal validates the session/request shaped kinds. Identity-only
// fields must stay empty there so a ready or reported receipt can never carry
// probe correlation into identity matching.
func parseSessionSignal(signal Signal) (Signal, error) {
	if signal.SessionNonce == "" || signal.Character == "" || signal.Realm == "" || signal.Sequence == 0 || signal.Sequence > 9007199254740991 {
		return signal, errors.New("bridge.invalid_signal")
	}
	if signal.ProbeNonce != "" || signal.ActorState != "" || signal.InputReason != "" {
		return signal, errors.New("bridge.invalid_identity_signal")
	}
	if signal.Kind != "ready" && signal.RequestID == "" {
		return signal, errors.New("bridge.signal_missing_request")
	}
	if signal.GUID != "" && (signal.Kind != "ready" && signal.Kind != "cleared" || !queueLabel(signal.GUID)) {
		return signal, errors.New("bridge.invalid_signal_guid")
	}
	if signal.Kind == "cleared" {
		if !queueHex(signal.CleanupNonce, 32) || signal.GUID == "" || signal.ReloadNonce != "" || signal.InputReady || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CodeAdler32 != "" || signal.ReportAdler32 != "" {
			return signal, errors.New("bridge.invalid_cleanup_signal")
		}
	} else if signal.CleanupNonce != "" {
		return signal, errors.New("bridge.unexpected_cleanup_nonce")
	}
	if signal.Kind == "loaded" && signal.CodeBytes == 0 || signal.Kind == "reported" && signal.ReportBytes == 0 {
		return signal, errors.New("bridge.signal_missing_payload_identity")
	}
	if signal.CodeBytes > 256<<10 || signal.ReportBytes > 512<<10 {
		return signal, errors.New("bridge.signal_payload_limit")
	}
	if signal.CodeBytes > 0 && !checksum(signal.CodeAdler32) || signal.ReportBytes > 0 && !checksum(signal.ReportAdler32) {
		return signal, errors.New("bridge.invalid_signal_checksum")
	}
	return signal, nil
}

// parseIdentitySignal validates the session-free identity marker. Sequence is
// fixed at zero because probeNonce, not sequence, correlates one host trigger
// with exactly one displayed receipt.
func parseIdentitySignal(signal Signal) (Signal, error) {
	if !queueHex(signal.ProbeNonce, 32) {
		return signal, errors.New("bridge.invalid_signal_probe_nonce")
	}
	switch signal.ActorState {
	case "ok", "no_actor", "actor_restricted":
	default:
		return signal, errors.New("bridge.invalid_signal_actor_state")
	}
	if signal.SessionNonce != "" || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CleanupNonce != "" ||
		signal.Sequence != 0 || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CodeAdler32 != "" || signal.ReportAdler32 != "" {
		return signal, errors.New("bridge.invalid_identity_signal")
	}
	if signal.ActorState == "ok" {
		if !queueLabel(signal.Character) || !queueLabel(signal.Realm) {
			return signal, errors.New("bridge.invalid_identity_actor")
		}
	} else if signal.Character != "" || signal.Realm != "" || signal.GUID != "" {
		return signal, errors.New("bridge.invalid_identity_actor")
	}
	if signal.GUID != "" && !queueLabel(signal.GUID) {
		return signal, errors.New("bridge.invalid_signal_guid")
	}
	if signal.InputReady && signal.InputReason != "" || signal.InputReason != "" && !inputReasonLabel(signal.InputReason) {
		return signal, errors.New("bridge.invalid_signal_input_reason")
	}
	return signal, nil
}

func inputReasonLabel(value string) bool {
	if len(value) < 7 || len(value) > 64 || !strings.HasPrefix(value, "input_") {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_') {
			return false
		}
	}
	return true
}

func (s Signal) Match(expected SignalExpectation) error {
	if s.Sequence > 9007199254740991 {
		return fmt.Errorf("%w: sequence outside Lua integer range", ErrSignalIdentity)
	}
	// An identity marker never satisfies a session expectation and vice versa,
	// even when the caller leaves Kind empty.
	if (s.Kind == "identity") != (expected.Kind == "identity") {
		return fmt.Errorf("%w: signal kind", ErrSignalIdentity)
	}
	// Identity matching is always probe-correlated; an uncorrelated identity
	// expectation could adopt whatever marker happens to be on screen.
	if expected.Kind == "identity" && !queueHex(expected.ProbeNonce, 32) {
		return fmt.Errorf("%w: missing probe nonce", ErrSignalIdentity)
	}
	if expected.RuntimeEpoch != 0 && s.RuntimeEpoch != expected.RuntimeEpoch {
		return fmt.Errorf("%w: runtime epoch", ErrSignalIdentity)
	}
	for _, pair := range [][2]string{{s.Release, expected.Release}, {s.Kind, expected.Kind}, {s.SessionNonce, expected.SessionNonce}, {s.RequestID, expected.RequestID}, {s.ReloadNonce, expected.ReloadNonce}, {s.CleanupNonce, expected.CleanupNonce}, {s.ProbeNonce, expected.ProbeNonce}, {s.ActorState, expected.ActorState}, {s.Character, expected.Character}, {s.Realm, expected.Realm}, {s.Product, expected.Product}, {s.Build, expected.Build}} {
		if pair[1] != "" && pair[0] != pair[1] {
			return ErrSignalIdentity
		}
	}
	if s.Kind == "identity" {
		// probeNonce correlation replaces sequence freshness for identity.
		if expected.AfterSequence != 0 {
			return fmt.Errorf("%w: stale sequence", ErrSignalIdentity)
		}
	} else if s.Sequence <= expected.AfterSequence {
		return fmt.Errorf("%w: stale sequence", ErrSignalIdentity)
	}
	if expected.RequireInputReady && !s.InputReady {
		return errors.New("bridge.input_not_ready")
	}
	return nil
}

func checksum(value string) bool {
	if len(value) != 8 {
		return false
	}
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
