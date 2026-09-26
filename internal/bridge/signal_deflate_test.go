package bridge

import (
	"bytes"
	"compress/flate"
	"encoding/json"
	"testing"
)

// ParseSignal takes the exact transmitted payload in bytes; desktop.DecodeSymbols
// is the one place that converts the QR reader's ISO-8859-1 text back to bytes
// (desktop.BytesFromSymbolText). The 0x1F marker documents a transport the
// shipped addon no longer produces, because a raw DEFLATE stream is not valid
// UTF-8 and a SavedVariables document must stay valid UTF-8. The host still
// accepts the marker so a receipt drawn and archived by an earlier build stays
// verifiable.
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

	parsed, err := ParseSignal(rawPayload)
	if err != nil {
		t.Fatalf("inflated parse: %v", err)
	}
	if parsed.Kind != "reported" || parsed.RequestID != signal.RequestID || parsed.ReportBytes != signal.ReportBytes {
		t.Fatalf("parsed mismatch: %+v", parsed)
	}

	// A damaged deflate stream must not parse as anything.
	damaged := append([]byte{0x1F, 0x00}, rawPayload[2:12]...)
	if _, err := ParseSignal(damaged); err == nil {
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
