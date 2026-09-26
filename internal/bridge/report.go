package bridge

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"hash/adler32"
	"unicode/utf8"
)

// VerifiedReport owns the exact persisted bytes, not a re-encoded JSON value.
// Verification is transport integrity, not authentication or an ACK. The
// executor must archive these bytes before advancing its durable operation.
type VerifiedReport struct {
	Receipt      Signal
	ReceiptBytes []byte
	Body         []byte
	SHA256       string
}

// VerifyReport joins independently observed identity, the original submitted
// code and persisted report bytes. code must come from the operation's immutable
// input, never from the saved report. Built-in requests have no submitted code.
//
// The receipt may omit the actor and build identity to save QR modules, so the
// caller's expectation is also the baseline: omitted fields are filled from it
// and every field it names is still compared, because Match checks a field the
// expectation sets. Verification therefore cannot pass on a receipt that
// contradicts the identity the caller independently observed.
func VerifyReport(receiptBytes, body, code []byte, expected SignalExpectation) (VerifiedReport, error) {
	var result VerifiedReport
	if expected.Kind != "reported" || expected.Release == "" || expected.SessionNonce == "" || expected.RequestID == "" || expected.Character == "" || expected.Realm == "" || expected.Product == "" || expected.Build == "" {
		return result, errors.New("bridge.incomplete_report_expectation")
	}
	receipt, err := ParseSignal(receiptBytes)
	if err != nil {
		return result, err
	}
	receipt = FillSignalIdentity(receipt, SignalIdentity{
		Release: expected.Release, Character: expected.Character, Realm: expected.Realm,
		Product: expected.Product, Build: expected.Build, SessionNonce: expected.SessionNonce,
	})
	if err := receipt.Match(expected); err != nil {
		return result, err
	}
	if len(code) > 256<<10 || len(body) == 0 || len(body) > 512<<10 {
		return result, errors.New("bridge.report_payload_limit")
	}
	if uint32(len(code)) != receipt.CodeBytes || (len(code) > 0 && byteChecksum(code) != receipt.CodeAdler32) || (len(code) == 0 && receipt.CodeAdler32 != "") {
		return result, errors.New("bridge.report_code_mismatch")
	}
	if uint32(len(body)) != receipt.ReportBytes || byteChecksum(body) != receipt.ReportAdler32 {
		return result, errors.New("bridge.report_body_mismatch")
	}
	if !utf8.Valid(body) || !json.Valid(body) {
		return result, errors.New("bridge.report_invalid_json")
	}
	if err := validateJSON(body, 32, 32768); err != nil {
		return result, err
	}
	digest := sha256.Sum256(body)
	return VerifiedReport{Receipt: receipt, ReceiptBytes: append([]byte(nil), receiptBytes...), Body: append([]byte(nil), body...), SHA256: hex.EncodeToString(digest[:])}, nil
}

func byteChecksum(data []byte) string { return fmt.Sprintf("%08x", adler32.Checksum(data)) }
