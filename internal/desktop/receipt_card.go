package desktop

import (
	"image"

	"github.com/makiuchi-d/gozxing"
	"github.com/makiuchi-d/gozxing/qrcode/decoder"
)

// Small UI-scaled modules can alternate between one and two physical pixels.
// The general detector can miss their finder patterns. The receipt's square
// white backing supplies a second geometric observation: sample module centres
// and the first covered pixels (fractional one-pixel modules have no centre pixel)
// for the bounded QR versions, then use the normal QR error-correcting decoder.
// This only recovers bytes; all identity, nonce and freshness checks stay above
// the desktop layer. No prior payload or expected result is used here.
func decodeReceiptCard(frame image.Image) []string {
	b := frame.Bounds()
	white := func(x, y int) bool {
		r, g, blue, _ := frame.At(x, y).RGBA()
		return r >= 0xf500 && g >= 0xf500 && blue >= 0xf500
	}
	attempts := 0
	// ReceiptView anchors near the top left. Bound both scanning and candidate
	// work even when a game frame contains large white panels or no receipt.
	for y := b.Min.Y; y < min(b.Max.Y, b.Min.Y+128); y++ {
		for x := b.Min.X; x < min(b.Max.X, b.Min.X+128); x++ {
			if !white(x, y) || (x > b.Min.X && white(x-1, y)) || (y > b.Min.Y && white(x, y-1)) {
				continue
			}
			right := x
			for right < min(b.Max.X, x+1025) && white(right, y) {
				right++
			}
			side := right - x
			// Nearby game text can touch the outermost white row. The quiet
			// zone gives us several rows with which to bound the actual card.
			for offset := 1; offset <= 3 && y+offset < b.Max.Y; offset++ {
				edge := x
				for edge < min(b.Max.X, x+side) && white(edge, y+offset) {
					edge++
				}
				side = min(side, edge-x)
			}
			right = x + side
			if side < 58 || side > 1024 || y+side > b.Max.Y {
				continue
			}
			border := true
			for i := 0; i < side; i++ {
				if !white(x+i, y+side-1) || !white(x, y+i) || !white(right-1, y+i) {
					border = false
					break
				}
			}
			if !border {
				continue
			}
			attempts++
			// A fractional final edge may rasterize one pixel inside its
			// mathematical bound. Try the adjacent extent, never arbitrary warps.
			for extent := side; extent <= side+1; extent++ {
				for version := 1; version <= 40; version++ {
					dimension := 17 + 4*version
					cells := dimension + 8 // four-module quiet zone on every edge
					if side*100 < cells*125 || side > cells*4 {
						continue
					}
					for _, firstPixel := range []bool{false, true} {
						matrix, _ := gozxing.NewBitMatrix(dimension, dimension)
						for row := 0; row < dimension; row++ {
							for col := 0; col < dimension; col++ {
								px := x + (2*(col+4)+1)*extent/(2*cells)
								py := y + (2*(row+4)+1)*extent/(2*cells)
								if firstPixel {
									px = x + ((col+4)*extent+cells-1)/cells
									py = y + ((row+4)*extent+cells-1)/cells
								}
								r, g, blue, _ := frame.At(px, py).RGBA()
								if (r+g+blue)/3 < 0x8000 {
									matrix.Set(col, row)
								}
							}
						}
						result, err := decoder.NewDecoder().Decode(matrix, map[gozxing.DecodeHintType]interface{}{gozxing.DecodeHintType_CHARACTER_SET: "ISO-8859-1"})
						if err == nil && len(result.GetText()) <= 4096 {
							return []string{result.GetText()}
						}
					}
				}
			}
			if attempts >= 4 {
				return []string{}
			}
		}
	}
	return []string{}
}
