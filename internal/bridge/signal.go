package bridge

import (
	"bytes"
	"compress/flate"
	"encoding/base64"
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

// Signal transport markers. A receipt is drawn into a QR symbol, so its size is
// the difference between a card the host can sample and one it cannot.
//
//   - signalDeflateMarker (0x1F) prefixes raw DEFLATE bytes. It is read for
//     compatibility with a receipt drawn by an earlier build; the shipped addon
//     no longer emits it, because a raw DEFLATE stream is not valid UTF-8 and
//     SavedVariables must stay valid UTF-8 for the host to read any state.
//   - signalEncodedMarker (0x1E) prefixes base64 text carrying raw DEFLATE
//     bytes. Base64 is pure ASCII, so the compressed transport stays valid
//     UTF-8 and survives both the QR byte mode and the SavedVariables document.
const (
	signalDeflateMarker = 0x1F
	signalEncodedMarker = 0x1E
)

// SignalIdentity is the actor and build identity a retained session already
// proved. A receipt on the wire carries only what correlates it to an operation
// and proves the payload digest; replaying the identity the host already holds
// in every symbol wastes QR modules and buys nothing, because the host compares
// the filled value against this same baseline before accepting the receipt.
//
// SessionNonce belongs here for the same reason and is the largest single win:
// every receipt of one session repeats the same 32-hex-character nonce, which is
// 40 bytes of a roughly 300-byte symbol. The host already stored it when it
// bound the session and compares it against the operation's expected value, so
// an omitted nonce is filled and still verified.
type SignalIdentity struct {
	Release, Character, Realm, GUID, Product, Build string
	SessionNonce                                    string
}

// FillSignalIdentity completes any identity field the wire omitted from the
// caller's baseline. A field the wire did carry is never overwritten: a signal
// that contradicts the baseline stays a mismatch for the caller's comparison
// rather than being silently repaired into agreement.
func FillSignalIdentity(signal Signal, baseline SignalIdentity) Signal {
	if signal.Release == "" {
		signal.Release = baseline.Release
	}
	if signal.Character == "" {
		signal.Character = baseline.Character
	}
	if signal.Realm == "" {
		signal.Realm = baseline.Realm
	}
	if signal.GUID == "" {
		signal.GUID = baseline.GUID
	}
	if signal.Product == "" {
		signal.Product = baseline.Product
	}
	if signal.Build == "" {
		signal.Build = baseline.Build
	}
	if signal.SessionNonce == "" {
		// The identity marker establishes the session; it never echoes one, and
		// the host has no nonce to lend it. Filling an empty marker from an empty
		// baseline is harmless, but filling it for a session-shaped receipt is
		// what lets the compact wire form work, so both go through the same rule.
		signal.SessionNonce = baseline.SessionNonce
	}
	return signal
}

func ParseSignal(data []byte) (Signal, error) {
	var signal Signal
	if len(data) == 0 || len(data) > 4096 {
		return signal, errors.New("bridge.signal_budget_or_encoding")
	}
	switch data[0] {
	case signalDeflateMarker:
		// Archived raw-DEFLATE transport: inflate the bytes after the marker.
		if len(data) < 2 {
			return signal, errors.New("bridge.signal_budget_or_encoding")
		}
		inflated, err := inflateSignal(data[1:])
		if err != nil {
			return signal, fmt.Errorf("%w: %v", errors.New("bridge.signal_budget_or_encoding"), err)
		}
		data = inflated
	case signalEncodedMarker:
		// Base64 text carrying raw DEFLATE bytes.
		if len(data) < 2 {
			return signal, errors.New("bridge.signal_budget_or_encoding")
		}
		compressed, err := base64.StdEncoding.DecodeString(string(data[1:]))
		if err != nil {
			return signal, fmt.Errorf("%w: %v", errors.New("bridge.signal_budget_or_encoding"), err)
		}
		inflated, err := inflateSignal(compressed)
		if err != nil {
			return signal, fmt.Errorf("%w: %v", errors.New("bridge.signal_budget_or_encoding"), err)
		}
		data = inflated
	}
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
	// The schema is the only field every kind must carry. Identity, reset and
	// cleared markers are session-free and re-establish the actor, so they are
	// allowed to omit the release and build; a session-shaped receipt is not,
	// and parseSessionSignal enforces that below.
	if signal.Schema != "lycheedev.signal.v1" {
		return signal, errors.New("bridge.invalid_signal")
	}
	if signal.RuntimeEpoch > 9007199254740991 {
		return signal, errors.New("bridge.invalid_runtime_epoch")
	}
	switch signal.Kind {
	case "identity":
		return parseIdentitySignal(signal)
	case "reset":
		return parseResetSignal(signal)
	case "ready", "loaded", "reported", "acknowledged", "cancelled", "cleared":
	default:
		return signal, errors.New("bridge.invalid_signal_kind")
	}
	if signal.RuntimeEpoch != 0 && signal.Kind != "ready" && signal.Kind != "cleared" {
		return signal, errors.New("bridge.invalid_runtime_epoch")
	}
	return parseSessionSignal(signal)
}

func inflateSignal(compressed []byte) ([]byte, error) {
	reader := flate.NewReader(bytes.NewReader(compressed))
	defer reader.Close()
	// Reject oversized output before allocating the full expansion.
	return io.ReadAll(io.LimitReader(reader, 4097))
}

// parseSessionSignal validates the session/request shaped kinds. Identity-only
// fields must stay empty there so a ready or reported receipt can never carry
// probe correlation into identity matching.
//
// The actor, build and session identity are optional on the wire: a retained
// session already proved them, so the caller fills them from its baseline with
// FillSignalIdentity and then compares them through Match, which still rejects a
// value that contradicts the baseline. The session nonce is the largest single
// saving, because every receipt of one session repeats the same 32 hex
// characters. Requiring these fields here only forced every symbol to repeat
// bytes the host already had, at the cost of QR modules. Any field the wire does
// carry must still be well formed.
func parseSessionSignal(signal Signal) (Signal, error) {
	if signal.Sequence == 0 || signal.Sequence > 9007199254740991 {
		return signal, errors.New("bridge.invalid_signal")
	}
	// A ready receipt establishes the session, so it must name one: it is the
	// only receipt the host sees before it can fill anything, and an omitted
	// nonce there would leave it correlating nothing. Every later receipt echoes
	// that session and may omit the nonce, which is what makes the compact wire
	// form work.
	if signal.Kind == "ready" && signal.SessionNonce == "" {
		return signal, errors.New("bridge.signal_missing_session")
	}
	if err := checkOptionalLabel(signal.SessionNonce); err != nil {
		return signal, err
	}
	// A session-shaped receipt still names the release, product and build it
	// belongs to; the host fills the actor from its baseline, but a missing
	// build identity is a malformed receipt, not a compact one.
	if signal.Release == "" || signal.Product == "" || signal.Build == "" {
		return signal, errors.New("bridge.invalid_signal")
	}
	if err := checkOptionalLabel(signal.Release); err != nil {
		return signal, err
	}
	if err := checkOptionalLabel(signal.Character); err != nil {
		return signal, err
	}
	if err := checkOptionalLabel(signal.Realm); err != nil {
		return signal, err
	}
	if err := checkOptionalLabel(signal.Product); err != nil {
		return signal, err
	}
	if err := checkOptionalLabel(signal.Build); err != nil {
		return signal, err
	}
	if signal.ProbeNonce != "" || signal.ActorState != "" || signal.InputReason != "" {
		return signal, errors.New("bridge.invalid_identity_signal")
	}
	if signal.Kind != "ready" && signal.RequestID == "" {
		return signal, errors.New("bridge.signal_missing_request")
	}
	// A GUID may appear on any session-shaped receipt: it is the actor the
	// session already proved, so the host fills it from its baseline when the
	// wire omits it and compares it when the caller names one.
	if signal.GUID != "" && !queueLabel(signal.GUID) {
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

// parseResetSignal validates the session-free recovery receipt. The actor is
// mandatory because the reset only ever tombstones that actor's own entries,
// and inputReady stays false: the receipt proves delivery, never readiness.
func parseResetSignal(signal Signal) (Signal, error) {
	if !queueHex(signal.ProbeNonce, 32) {
		return signal, errors.New("bridge.invalid_signal_probe_nonce")
	}
	if signal.SessionNonce != "" || signal.RequestID != "" || signal.ReloadNonce != "" || signal.CleanupNonce != "" ||
		signal.Sequence != 0 || signal.CodeBytes != 0 || signal.ReportBytes != 0 || signal.CodeAdler32 != "" || signal.ReportAdler32 != "" ||
		signal.ActorState != "" || signal.InputReason != "" || signal.InputReady {
		return signal, errors.New("bridge.invalid_reset_signal")
	}
	if !queueLabel(signal.Character) || !queueLabel(signal.Realm) || !queueLabel(signal.GUID) {
		return signal, errors.New("bridge.invalid_reset_actor")
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

// checkOptionalLabel accepts an omitted field but rejects a malformed one, so
// dropping identity bytes from the wire never weakens validation of the bytes
// that are still transmitted.
func checkOptionalLabel(value string) error {
	if value == "" {
		return nil
	}
	if !queueLabel(value) {
		return errors.New("bridge.invalid_signal_label")
	}
	return nil
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
	if s.Kind == "identity" || s.Kind == "reset" {
		// Session-free discovery receipts replace sequence freshness with
		// mandatory nonce correlation enforced by their discovery entries.
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
