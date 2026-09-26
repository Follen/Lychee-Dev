package desktop

import (
	"bytes"
	"testing"
)

// The QR reader is required to read byte mode with ISO-8859-1, so DecodeSymbols
// returns one rune per transmitted byte and BytesFromSymbolText reverses exactly
// that one step. Two properties matter and have both been broken in practice:
// a UTF-8 payload (Chinese character and realm names) must survive, and a binary
// payload (a raw DEFLATE stream) must survive too, because ParseSignal still
// accepts the archived 0x1F transport. Applying the conversion twice truncates
// every byte to its low bits; skipping it leaves text that is not valid UTF-8.
func TestBytesFromSymbolTextRoundTripsUTF8AndBinary(t *testing.T) {
	cases := []struct {
		name    string
		payload []byte
	}{
		{name: "ascii", payload: []byte(`{"kind":"ready","character":"Follen"}`)},
		{name: "utf8", payload: []byte(`{"kind":"ready","character":"Follen","realm":"匕首岭"}`)},
		{name: "deflate-marker", payload: append([]byte{0x1F}, 0x00, 0x8c, 0x95, 0xe9, 0xa6, 0x96, 0xff)},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			// What gozxing hands back for a byte-mode symbol: each transmitted
			// byte becomes one rune, which Go then spells as UTF-8.
			runes := make([]rune, 0, len(test.payload))
			for _, b := range test.payload {
				runes = append(runes, rune(b))
			}
			text := string(runes)
			got := BytesFromSymbolText(text)
			if !bytes.Equal(got, test.payload) {
				t.Fatalf("round trip = % x, want % x", got, test.payload)
			}
			// For a non-ASCII payload the reader's text form is a different byte
			// sequence than the payload, which is exactly the defect this
			// function exists to prevent. Pure ASCII is indistinguishable.
			nonASCII := false
			for _, b := range test.payload {
				if b > 0x7f {
					nonASCII = true
					break
				}
			}
			if nonASCII && bytes.Equal([]byte(text), test.payload) {
				t.Fatal("reader text form matched a non-ASCII payload")
			}
		})
	}
}
