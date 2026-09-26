package desktop

import (
	"errors"
	"image"
	"sort"

	"github.com/makiuchi-d/gozxing"
	multiqr "github.com/makiuchi-d/gozxing/multi/qrcode"
)

// DecodeSymbols reads only QR symbols and returns each payload as the exact
// transmitted byte sequence, re-spelled as an ISO-8859-1 text so that binary
// payloads (a deflate stream) and UTF-8 payloads (Chinese character and realm
// names) both survive. Callers that need raw bytes apply BytesFromSymbolText;
// bridge.ParseSignal does. A valid frame with no detected symbol returns an
// empty list; callers must not confuse a missing frame with this.
func DecodeSymbols(frame image.Image) ([]string, error) {
	if frame == nil {
		return nil, errors.New("desktop.missing_frame")
	}
	bounds := frame.Bounds()
	if bounds.Empty() || bounds.Dx() > 4096 || bounds.Dy() > 4096 {
		return nil, errors.New("desktop.invalid_decode_region")
	}
	bitmap, err := gozxing.NewBinaryBitmapFromImage(frame)
	if err != nil {
		return nil, err
	}
	reader := multiqr.NewQRCodeMultiReader()
	// CHARL_8859_1 keeps the QR byte payload byte-exact: compressed signal
	// payloads are binary (deflate) and a UTF-8 payload must not be re-encoded.
	// Each returned rune is therefore one transmitted byte, never a decoded
	// character; see BytesFromSymbolText.
	results, err := reader.DecodeMultiple(bitmap, map[gozxing.DecodeHintType]interface{}{gozxing.DecodeHintType_TRY_HARDER: true, gozxing.DecodeHintType_CHARACTER_SET: "ISO-8859-1"})
	if err != nil {
		var missing gozxing.NotFoundException
		if errors.As(err, &missing) {
			return []string{}, nil
		}
		return nil, err
	}
	unique := make(map[string]bool)
	for _, result := range results {
		text := result.GetText()
		if len(text) > 4096 {
			return nil, errors.New("desktop.invalid_symbol_payload")
		}
		unique[text] = true
	}
	symbols := make([]string, 0, len(unique))
	for text := range unique {
		symbols = append(symbols, text)
	}
	sort.Strings(symbols)
	return symbols, nil
}

// BytesFromSymbolText converts one DecodeSymbols text back into the exact
// transmitted bytes. DecodeSymbols reads byte mode with ISO-8859-1, so a
// non-ASCII payload arrives as one rune per byte, UTF-8 encoded; this reverses
// that single step. Applying it twice corrupts the payload, and skipping it
// leaves the reader's text form, which is not valid UTF-8.
func BytesFromSymbolText(text string) []byte {
	runes := []rune(text)
	out := make([]byte, len(runes))
	for i, r := range runes {
		out[i] = byte(r)
	}
	return out
}
