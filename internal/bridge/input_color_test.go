package bridge

import (
	"encoding/json"
	"image"
	"image/color"
	"os"
	"testing"
)

var signalColors = map[string]color.NRGBA{"red": {255, 0, 0, 255}, "green": {0, 255, 0, 255}, "blue": {0, 0, 255, 255}, "white": {255, 255, 255, 255}, "black": {0, 0, 0, 255}}

func signalImage(a, b, h string) *image.NRGBA {
	im := image.NewNRGBA(image.Rect(7, 9, 13, 11))
	for i, name := range []string{a, b, h} {
		for y := 9; y < 11; y++ {
			for x := 7 + i*2; x < 9+i*2; x++ {
				im.SetNRGBA(x, y, signalColors[name])
			}
		}
	}
	return im
}
func TestInputSignalCodebookAndSingleSymbolErrors(t *testing.T) {
	raw, err := os.ReadFile("../../protocol/input-color/golden.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Capability string
		States     []struct {
			State  string
			Colors []string
		}
	}
	if err = json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	if fixture.Capability != InputSignalCapability || len(fixture.States) != 4 {
		t.Fatal(fixture)
	}
	valid := map[[2]string]string{}
	for _, s := range fixture.States {
		valid[[2]string{s.Colors[0], s.Colors[1]}] = s.State
	}
	for a := range signalColors {
		for b := range signalColors {
			for _, h := range []string{"black", "white"} {
				got, err := DecodeInputSignal(signalImage(a, b, h))
				want, ok := valid[[2]string{a, b}]
				if ok && (err != nil || got.State != want || got.Heartbeat != (h == "white")) {
					t.Fatalf("%s/%s/%s: %+v %v", a, b, h, got, err)
				}
				if !ok && err == nil {
					t.Fatalf("illegal code %s/%s accepted", a, b)
				}
			}
		}
	}
	for pair := range valid {
		for c := range signalColors {
			for i := 0; i < 2; i++ {
				mut := pair
				if mut[i] == c {
					continue
				}
				mut[i] = c
				if _, err := DecodeInputSignal(signalImage(mut[0], mut[1], "white")); err == nil {
					t.Fatalf("single symbol error became valid: %v -> %v", pair, mut)
				}
			}
		}
	}
}
func TestInputSignalRejectsUnreliablePixels(t *testing.T) {
	for _, c := range []color.NRGBA{{128, 128, 128, 255}, {0, 255, 0, 249}, {0, 0, 255, 255}, {32, 255, 0, 255}, {0, 223, 0, 255}} {
		for y := 9; y < 11; y++ {
			for x := 7; x < 9; x++ {
				im := signalImage("green", "white", "black")
				im.SetNRGBA(x, y, c)
				if _, err := DecodeInputSignal(im); err == nil {
					t.Fatal("unreliable pixel accepted", x, y, c)
				}
			}
		}
	}
	for _, h := range []string{"red", "green", "blue"} {
		if _, err := DecodeInputSignal(signalImage("green", "white", h)); err == nil {
			t.Fatal(h)
		}
	}
	for _, im := range []*image.NRGBA{nil, image.NewNRGBA(image.Rect(0, 0, 5, 2)), image.NewNRGBA(image.Rect(0, 0, 7, 2)), image.NewNRGBA(image.Rect(0, 0, 6, 3))} {
		if _, err := DecodeInputSignal(im); err == nil {
			t.Fatal("bad extent")
		}
	}
	im := signalImage("green", "white", "black")
	for y := 9; y < 11; y++ {
		for x := 7; x < 9; x++ {
			im.SetNRGBA(x, y, color.NRGBA{31, 224, 31, 250})
		}
	}
	if _, err := DecodeInputSignal(im); err != nil {
		t.Fatal("threshold boundary rejected", err)
	}
}
