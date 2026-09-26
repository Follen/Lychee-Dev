package desktop

import (
	"image"
	"image/color"
	"strings"
	"testing"

	"github.com/makiuchi-d/gozxing/qrcode/decoder"
	"github.com/makiuchi-d/gozxing/qrcode/encoder"
)

func TestReceiptCardFractionalModuleSampling(t *testing.T) {
	payload := `{"kind":"ready","realm":"祈福","padding":"` + strings.Repeat("a", 300) + `"}`
	qr, err := encoder.Encoder_encodeWithoutHint(payload, decoder.ErrorCorrectionLevel_M)
	if err != nil {
		t.Fatal(err)
	}
	m := qr.GetMatrix()
	cells := m.GetWidth() + 8
	for _, factor := range []float64{2, 2.35, 2.625, 3.1} {
		side := int(float64(cells) * factor)
		frame := image.NewNRGBA(image.Rect(0, 0, 600, 500))
		for y := 0; y < side; y++ {
			for x := 0; x < side; x++ {
				col, row := x*cells/side-4, y*cells/side-4
				v := uint8(255)
				if col >= 0 && row >= 0 && col < m.GetWidth() && row < m.GetHeight() && m.Get(col, row) == 1 {
					v = 0
				}
				frame.SetNRGBA(21+x, 21+y, color.NRGBA{v, v, v, 255})
			}
		}
		// Bright neighbouring UI text touches only the outer rows, as seen
		// beside the real Classic identity card. It must not enlarge the grid.
		for x := 21 + side; x < 21+side+3; x++ {
			frame.SetNRGBA(x, 21, color.NRGBA{255, 255, 255, 255})
		}
		for _, decode := range []func(image.Image) []string{decodeReceiptCard, func(i image.Image) []string {
			got, err := DecodeSymbols(i)
			if err != nil {
				t.Fatal(err)
			}
			return got
		}} {
			got := decode(frame)
			if len(got) != 1 || string(BytesFromSymbolText(got[0])) != payload {
				t.Fatalf("factor %v: %q", factor, got)
			}
		}
	}
}

func TestReceiptCardDoesNotInventPayloadFromWhitePanels(t *testing.T) {
	frame := image.NewNRGBA(image.Rect(0, 0, 512, 512))
	for y := 21; y < 245; y++ {
		for x := 21; x < 245; x++ {
			frame.SetNRGBA(x, y, color.NRGBA{255, 255, 255, 255})
		}
	}
	if got := decodeReceiptCard(frame); len(got) != 0 {
		t.Fatalf("white panel decoded: %q", got)
	}
	if got := decodeReceiptCard(frame.SubImage(image.Rect(0, 0, 100, 100))); len(got) != 0 {
		t.Fatalf("clipped panel decoded: %q", got)
	}
}
