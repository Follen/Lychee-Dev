//go:build windows && amd64 && lycheedev_input_lab

package main

import (
	"github.com/makiuchi-d/gozxing/qrcode/decoder"
	"github.com/makiuchi-d/gozxing/qrcode/encoder"
	"image"
	"image/color"
	"testing"
)

func TestLabCardBelowProductionReceipt(t *testing.T) {
	payload := "LDIL1|0123456789abcdef|69933|abcd0123|1|binding_conflict|-|00000001|0|0|0"
	qr, err := encoder.Encoder_encodeWithoutHint(payload, decoder.ErrorCorrectionLevel_M)
	if err != nil {
		t.Fatal(err)
	}
	m := qr.GetMatrix()
	cells := m.GetWidth() + 8
	for _, factor := range []float64{1.42, 1.6, 1.8, 2, 2.35, 2.625, 3.1} {
		side := int(float64(cells) * factor)
		frame := image.NewNRGBA(image.Rect(0, 0, 1024, 1024))
		for y := 0; y < side; y++ {
			for x := 0; x < side; x++ {
				col, row := x*cells/side-4, y*cells/side-4
				v := uint8(255)
				if col >= 0 && row >= 0 && col < m.GetWidth() && row < m.GetHeight() && m.Get(col, row) == 1 {
					v = 0
				}
				frame.SetNRGBA(21+x, 683+y, color.NRGBA{v, v, v, 255})
			}
		}
		// Bright neighbouring UI text touches only the outer rows, as seen
		// beside the real Classic identity card. It must not enlarge the grid.
		for x := 21 + side; x < 21+side+3; x++ {
			frame.SetNRGBA(x, 683, color.NRGBA{255, 255, 255, 255})
		}
		for _, decode := range []func(image.Image) []string{func(i image.Image) []string {
			got, err := decodeLabSymbols(i)
			if err != nil {
				t.Fatal(err)
			}
			return got
		}} {
			got := decode(frame)
			if len(got) != 1 || got[0] != payload {
				t.Fatalf("factor %v: %q", factor, got)
			}
		}
	}
}
