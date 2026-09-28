package desktop

import (
	"image"
	"image/color"
	"image/draw"
	"testing"
)

func TestReloadPatchSingleSquare(t *testing.T) {
	for phase, c := range []color.NRGBA{{255, 0, 0, 255}, {0, 255, 0, 255}, {0, 0, 255, 255}} {
		img := image.NewNRGBA(image.Rect(0, 0, 120, 120))
		draw.Draw(img, img.Bounds(), &image.Uniform{color.NRGBA{100, 100, 100, 255}}, image.Point{}, draw.Src)
		draw.Draw(img, image.Rect(4, 4, 16, 16), &image.Uniform{color.Black}, image.Point{}, draw.Src)
		draw.Draw(img, image.Rect(6, 6, 14, 14), &image.Uniform{c}, image.Point{}, draw.Src)
		result, ok := ReadReloadPatch(img)
		if !ok || result.Phase != phase {
			t.Fatalf("phase %d: %+v %v", phase, result, ok)
		}
		draw.Draw(img, image.Rect(4, 4, 16, 16), &image.Uniform{c}, image.Point{}, draw.Src)
		if _, ok = ReadReloadPatch(img); ok {
			t.Fatal("accepted unframed square")
		}
	}
	if _, ok := ReadReloadPatch(nil); ok {
		t.Fatal("nil")
	}
}

func TestReloadPatchScaledThinBorder(t *testing.T) {
	// Titan at UI scale .71: physical WGC pixels are [4,10) inside
	// [3,11), not the nominal 8-pixel patch with a two-pixel surround.
	// Forever renders a 5x5 square at this client's smaller scale.
	for size := 4; size <= 12; size++ {
		img := image.NewNRGBA(image.Rect(0, 0, 120, 120))
		draw.Draw(img, img.Bounds(), &image.Uniform{color.NRGBA{100, 100, 100, 255}}, image.Point{}, draw.Src)
		draw.Draw(img, image.Rect(3, 3, 5+size, 5+size), &image.Uniform{color.Black}, image.Point{}, draw.Src)
		draw.Draw(img, image.Rect(4, 4, 4+size, 4+size), &image.Uniform{color.NRGBA{0, 0, 255, 255}}, image.Point{}, draw.Src)
		if got, ok := ReadReloadPatch(img); !ok || got.Phase != 2 {
			t.Fatalf("size %d: %+v %v", size, got, ok)
		}
		img.SetNRGBA(4+size/2, 3, color.NRGBA{100, 100, 100, 255})
		if _, ok := ReadReloadPatch(img); ok {
			t.Fatal("accepted broken surround")
		}
	}
}
