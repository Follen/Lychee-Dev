package live

import (
	"github.com/follenfang/lycheedev/internal/desktop"
	"image"
	"image/color"
	"image/draw"
	"testing"
	"time"
)

func beaconFrame(phase int, ticks int64) *desktop.CapturedFrame {
	p := image.NewNRGBA(image.Rect(0, 0, 80, 40))
	colors := []color.NRGBA{{255, 0, 0, 255}, {0, 255, 0, 255}, {0, 0, 255, 255}}
	for i := 0; i < 3; i++ {
		draw.Draw(p, image.Rect(6+i*10, 6, 14+i*10, 14), &image.Uniform{colors[(phase+i)%3]}, image.Point{}, draw.Src)
	}
	return &desktop.CapturedFrame{NRGBA: p, SystemTicks: ticks, ObservedAt: time.Now()}
}

func TestStartupBeaconRequiresFreshRotation(t *testing.T) {
	var o startupBeaconObserver
	if o.Observe(beaconFrame(0, 1)) || o.Observe(beaconFrame(0, 2)) {
		t.Fatal("static image signalled readiness")
	}
	if o.Observe(beaconFrame(1, 2)) {
		t.Fatal("repeated capture tick was fresh")
	}
	if o.Observe(beaconFrame(1, 3)) || !o.Observe(beaconFrame(2, 4)) {
		t.Fatal("ordered rotation not detected")
	}
	o = startupBeaconObserver{}
	f := beaconFrame(0, 1)
	f.ObservedAt = time.Now().Add(-2 * time.Second)
	if o.Observe(f) || o.seen {
		t.Fatal("stale frame was used")
	}
	if o.Observe(beaconFrame(2, 2)) || o.Observe(beaconFrame(1, 3)) {
		t.Fatal("reverse rotation accepted")
	}
}
