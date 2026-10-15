package qrfixture

import (
	"image"
	"image/color"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
)

func TestLuaReadyReceiptAtSmallFractionalScale(t *testing.T) {
	payload := `{"schema":"lycheedev.signal.v1","release":"2.0.5","kind":"ready","sessionNonce":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","requestId":"","product":"retail","build":"12.1.0.69933","character":"Paladin","realm":"Realm","guid":"Player-1-123","sequence":1,"inputReady":true,"runtimeEpoch":1}`
	modules := runLuaEncoder(t, findLua51(t), "harness.lua", "../../addon/Bridge/MatrixSymbol.lua", []byte(payload))
	cells := len(modules) + 8
	for _, factor := range []float64{1.28, 1.42, 1.6, 1.8, 2, 2.35, 3} {
		side := int(float64(cells) * factor)
		frame := image.NewNRGBA(image.Rect(0, 0, 600, 500))
		for y := 0; y < side; y++ {
			for x := 0; x < side; x++ {
				col, row := x*cells/side-4, y*cells/side-4
				value := uint8(255)
				if col >= 0 && row >= 0 && col < len(modules) && row < len(modules) && modules[row][col] {
					value = 0
				}
				frame.SetNRGBA(21+x, 21+y, color.NRGBA{value, value, value, 255})
			}
		}
		got, err := desktop.DecodeSymbols(frame)
		if err != nil || len(got) != 1 || string(desktop.BytesFromSymbolText(got[0])) != payload {
			t.Fatalf("factor %v: %q %v", factor, got, err)
		}
	}
}
