package desktop

import (
	"image"
	"image/color"
	"image/draw"
	"testing"
)

func beaconImage(phase, offset int) *image.NRGBA {
	p := image.NewNRGBA(image.Rect(0, 0, 160, 100))
	draw.Draw(p, p.Bounds(), &image.Uniform{color.NRGBA{60, 70, 80, 255}}, image.Point{}, draw.Src)
	draw.Draw(p, image.Rect(4+offset, 4, 36+offset, 16), image.Black, image.Point{}, draw.Src)
	colors := []color.NRGBA{{255, 0, 0, 255}, {0, 255, 0, 255}, {0, 0, 255, 255}}
	for i := 0; i < 3; i++ {
		draw.Draw(p, image.Rect(6+offset+i*10, 6, 14+offset+i*10, 14), &image.Uniform{colors[(phase+i)%3]}, image.Point{}, draw.Src)
	}
	return p
}

func TestStartupBeaconPattern(t *testing.T) {
	for phase := 0; phase < 3; phase++ {
		for _, offset := range []int{0, 8, 32} {
			got, ok := ReadStartupBeacon(beaconImage(phase, offset))
			if !ok || got.Phase != phase {
				t.Fatalf("phase=%d offset=%d: %+v %v", phase, offset, got, ok)
			}
		}
	}
	p := beaconImage(0, 0)
	draw.Draw(p, image.Rect(16, 6, 24, 14), image.White, image.Point{}, draw.Src)
	if _, ok := ReadStartupBeacon(p); ok {
		t.Fatal("accepted wrong color")
	}
	for _, p := range []*image.NRGBA{nil, image.NewNRGBA(image.Rect(0, 0, 1, 1)), image.NewNRGBA(image.Rect(0, 0, 160, 100))} {
		if _, ok := ReadStartupBeacon(p); ok {
			t.Fatal("accepted missing beacon")
		}
	}
}
