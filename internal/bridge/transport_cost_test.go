package bridge

import (
	"bytes"
	"compress/flate"
	"encoding/base64"
	"encoding/json"
	"testing"
)

// The compact wire form drops identity the session already proved. This measures
// what is left, so the transport choice is made on numbers rather than taste.
//
// Raw DEFLATE behind the archived 0x1F marker is the smallest transport, and it
// is unusable: a raw DEFLATE stream is not valid UTF-8, and SavedVariables must
// stay valid UTF-8 for the host to read any toolkit state. Base64 behind 0x1E
// keeps the transport ASCII and therefore valid UTF-8, at the cost of the
// encoding's expansion.
func TestTransportCostOfOneRealReceipt(t *testing.T) {
	signal := Signal{
		Schema: "lycheedev.signal.v1", Release: "2.0.2", Kind: "reported",
		SessionNonce: "26161b3b1bd87b850000000000000001",
		RequestID:    "REQ-3b1f0a14435c866f43abcff4a2f5bbcd",
		Character:    "次年雪", Realm: "祈福",
		Product: "classic", Build: "5.5.4.69934",
		Sequence: 8, CodeBytes: 202, CodeAdler32: "00b941f2",
		ReportBytes: 101, ReportAdler32: "faf021a6",
	}
	full, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	compact := signal
	compact.Character, compact.Realm, compact.GUID = "", "", ""
	compactBytes, err := json.Marshal(compact)
	if err != nil {
		t.Fatal(err)
	}

	var buf bytes.Buffer
	writer, err := flate.NewWriter(&buf, flate.BestCompression)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write(compactBytes); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	deflated := buf.Bytes()
	base64Transport := append([]byte{signalEncodedMarker}, []byte(base64.StdEncoding.EncodeToString(deflated))...)

	// The recorded sizes are the contract for this choice; a change that moves
	// them should be a deliberate decision, not a silent drift.
	if len(full) != 370 {
		t.Fatalf("full receipt = %d bytes, want 370", len(full))
	}
	if len(compactBytes) != 329 {
		t.Fatalf("compact receipt = %d bytes, want 329", len(compactBytes))
	}
	if len(base64Transport) != 309 {
		t.Fatalf("base64 transport = %d bytes, want 309", len(base64Transport))
	}
	// Dropping identity is worth 11%; the entire deflate transport on top of it
	// is worth another 6%, because base64 pays back a third of what deflate saves.
	if saved := len(full) - len(compactBytes); saved != 41 {
		t.Fatalf("identity fields saved %d bytes, want 41", saved)
	}
	if saved := len(compactBytes) - len(base64Transport); saved != 20 {
		t.Fatalf("base64 transport saved %d bytes, want 20", saved)
	}
}

// The base64 transport must decode back to the exact document it carried, so the
// option stays available and verified even while the shipped addon sends plain
// text.
func TestBase64TransportRoundTrips(t *testing.T) {
	signal := Signal{
		Schema: "lycheedev.signal.v1", Release: "2.0.2", Kind: "reported",
		SessionNonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", RequestID: "REQ-" + string(mkRepeat('a', 29)),
		Character: "次年雪", Realm: "祈福", Product: "classic", Build: "5.5.4.69934",
		Sequence: 5, CodeBytes: 18, CodeAdler32: "450b06ec", ReportBytes: 13, ReportAdler32: "209c046d",
	}
	plain, err := json.Marshal(signal)
	if err != nil {
		t.Fatal(err)
	}
	var buf bytes.Buffer
	writer, err := flate.NewWriter(&buf, flate.BestCompression)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write(plain); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	transport := append([]byte{signalEncodedMarker}, []byte(base64.StdEncoding.EncodeToString(buf.Bytes()))...)

	parsed, err := ParseSignal(transport)
	if err != nil {
		t.Fatalf("base64 transport parse: %v", err)
	}
	if parsed.Character != signal.Character || parsed.Realm != signal.Realm {
		t.Fatalf("non-ASCII identity lost: %+v", parsed)
	}
	if parsed != signal {
		t.Fatalf("round trip mismatch:\n got %+v\nwant %+v", parsed, signal)
	}
}
