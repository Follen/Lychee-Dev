package desktop

import (
	"errors"
	"image"
	"sort"
	"unicode/utf8"

	"github.com/makiuchi-d/gozxing"
	multiqr "github.com/makiuchi-d/gozxing/multi/qrcode"
)

// DecodeSymbols reads only QR symbols. A valid frame with no detected symbol
// returns an empty list; callers must not confuse a missing frame with this.
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
	results, err := reader.DecodeMultiple(bitmap, map[gozxing.DecodeHintType]interface{}{gozxing.DecodeHintType_TRY_HARDER: true, gozxing.DecodeHintType_CHARACTER_SET: "UTF-8"})
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
		if len(text) > 4096 || !utf8.ValidString(text) {
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
