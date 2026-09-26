package bridge

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"unicode/utf8"
)

// ParseOpticalSignals expands one optical symbol into its independent receipts.
// Only the display uses this envelope; stored reports remain ordinary signals.
// A readiness tuple explicitly names its nonce, sequence and runtime epoch.
// It shares actor/build identity with the receipt, never request or digest data.
func ParseOpticalSignals(data []byte) ([]Signal, error) {
	if signal, err := ParseSignal(data); err == nil {
		return []Signal{signal}, nil
	}
	invalid := errors.New("bridge.invalid_receipt_pair")
	if len(data) == 0 || len(data) > 4096 || !utf8.Valid(data) {
		return nil, invalid
	}
	if err := validateJSON(data, 3, 40); err != nil {
		return nil, err
	}
	var wire struct {
		Schema  string            `json:"schema"`
		Receipt json.RawMessage   `json:"receipt"`
		Ready   []json.RawMessage `json:"ready"`
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&wire); err != nil || decoder.Decode(new(any)) != io.EOF || wire.Schema != "lycheedev.receipt.v1" || len(wire.Ready) != 3 {
		return nil, invalid
	}
	receipt, err := ParseSignal(wire.Receipt)
	if err != nil {
		return nil, err
	}
	switch receipt.Kind {
	case "loaded", "reported", "acknowledged", "cancelled":
	default:
		return nil, invalid
	}
	ready := Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: receipt.Release,
		Product: receipt.Product, Build: receipt.Build, Character: receipt.Character,
		Realm: receipt.Realm, GUID: receipt.GUID, InputReady: true}
	if json.Unmarshal(wire.Ready[0], &ready.SessionNonce) != nil ||
		json.Unmarshal(wire.Ready[1], &ready.Sequence) != nil ||
		json.Unmarshal(wire.Ready[2], &ready.RuntimeEpoch) != nil ||
		!queueHex(ready.SessionNonce, 32) || ready.Sequence <= receipt.Sequence ||
		ready.Sequence > 9007199254740991 || ready.RuntimeEpoch == 0 || ready.RuntimeEpoch > 9007199254740991 ||
		(receipt.SessionNonce != "" && receipt.SessionNonce != ready.SessionNonce) {
		return nil, invalid
	}
	// The explicit tuple binds even a compact report to its actual session;
	// a contradictory nonce must survive to the caller's normal Match check.
	if receipt.SessionNonce == "" {
		receipt.SessionNonce = ready.SessionNonce
	}
	return []Signal{receipt, ready}, nil
}
