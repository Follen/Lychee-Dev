package desktop

import (
	"image"
	"testing"
)

func TestResolveCaptureAreaBoundsDefaultAndPreservesExplicit(t *testing.T) {
	for _, size := range []image.Point{{5120, 1440}, {7680, 4320}, {800, 600}} {
		got, err := ResolveCaptureArea(image.Rectangle{}, size)
		want := image.Rect(0, 0, min(size.X, 1024), min(size.Y, 1024))
		if err != nil || got != want {
			t.Fatalf("%v: %v %v", size, got, err)
		}
	}
	explicit := image.Rect(12, 18, 640, 480)
	got, err := ResolveCaptureArea(explicit, image.Pt(5120, 1440))
	if err != nil || got != explicit {
		t.Fatal(got, err)
	}
	for _, size := range []image.Point{{0, 100}, {16385, 100}} {
		if _, err := ResolveCaptureArea(image.Rectangle{}, size); err == nil {
			t.Fatal(size)
		}
	}
	if _, err := ResolveCaptureArea(image.Rect(0, 0, 4097, 400), image.Pt(5120, 1440)); err == nil {
		t.Fatal("unbounded explicit region")
	}
	if _, err := ResolveCaptureArea(explicit, image.Pt(400, 300)); err == nil {
		t.Fatal("region outside window")
	}
}

func TestExplicitWholeWindowIsNotAutomaticROI(t *testing.T) {
	got, err := ResolveCaptureArea(WholeWindowCapture(), image.Pt(1920, 1080))
	if err != nil || got != image.Rect(0, 0, 1920, 1080) {
		t.Fatal(got, err)
	}
	if _, err := ResolveCaptureArea(WholeWindowCapture(), image.Pt(5120, 1440)); err == nil {
		t.Fatal("oversized explicit window silently cropped")
	}
}
