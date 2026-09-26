package bridge

import (
	"bytes"
	"compress/flate"
	"encoding/json"
	"testing"
)

// The addon deflates larger signal payloads behind the 0x1F marker to keep
// QR module density low on engines with soft UI rendering. The QR decoder
// expands each raw byte to one ISO-8859-1 rune in the decoded text; the host
// must convert back to raw bytes before inflating, and reject damaged
// deflate streams.
func TestParseSignalInflatesDeflatedPayload(t *testing.T) {
	signal := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.2", Kind: "reported",
		SessionNonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", RequestID: "REQ-" + string(mkRepeat('a', 29)),
		Character: "Paladin", Realm: "Realm", Product: "retail", Build: "12.1.0.69875",
		Sequence: 5, ReportBytes: 101, ReportAdler32: "faf021a6"}
	jsonBytes, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	var buf bytes.Buffer
	writer, err := flate.NewWriter(&buf, flate.DefaultCompression)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write(jsonBytes); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	rawPayload := append([]byte{0x1F}, buf.Bytes()...)

	// Simulate the QR decode round trip: gozxing maps each raw byte to one
	// ISO-8859-1 rune and returns the text as a UTF-8 encoded Go string.
	runes := make([]rune, len(rawPayload))
	for i, b := range rawPayload {
		runes[i] = rune(b)
	}
	text := string(runes)
	data := []byte(text)

	parsed, err := ParseSignal(data)
	if err != nil {
		t.Fatalf("inflated parse: %v", err)
	}
	if parsed.Kind != "reported" || parsed.RequestID != signal.RequestID || parsed.ReportBytes != signal.ReportBytes {
		t.Fatalf("parsed mismatch: %+v", parsed)
	}

	// A damaged deflate stream must not parse as anything.
	damaged := append([]byte{0x1F, 0x00}, rawPayload[2:12]...)
	damagedRunes := make([]rune, len(damaged))
	for i, b := range damaged {
		damagedRunes[i] = rune(b)
	}
	if _, err := ParseSignal([]byte(string(damagedRunes))); err == nil {
		t.Fatal("damaged payload accepted")
	}

	// Uncompressed JSON signals keep parsing exactly as before.
	plain, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ParseSignal(plain); err != nil {
		t.Fatalf("plain parse broke: %v", err)
	}
}

func mkRepeat(r rune, count int) []rune {
	out := make([]rune, count)
	for i := range out {
		out[i] = r
	}
	return out
}
