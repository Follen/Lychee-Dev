package desktop

import "image"

// StartupBeacon is a render hint only. It carries no identity, epoch or input
// permission; callers must still complete the normal correlated handshake.
type StartupBeacon struct{ X, Y, Pitch, Phase int }

func ReadStartupBeacon(p *image.NRGBA) (StartupBeacon, bool) {
	if p == nil {
		return StartupBeacon{}, false
	}
	b := p.Bounds()
	colorAt := func(x, y int) int {
		if !image.Pt(x, y).In(b) {
			return -1
		}
		c := p.NRGBAAt(x, y)
		if c.R >= 160 && c.G <= 90 && c.B <= 90 {
			return 0
		}
		if c.G >= 160 && c.R <= 90 && c.B <= 90 {
			return 1
		}
		if c.B >= 160 && c.R <= 90 && c.G <= 90 {
			return 2
		}
		return -1
	}
	black := func(x, y int) bool {
		if !image.Pt(x, y).In(b) {
			return false
		}
		c := p.NRGBAAt(x, y)
		return c.R < 50 && c.G < 50 && c.B < 50
	}
	// Search only the small top-left header, allowing native window borders and
	// UI-scale rounding. Uniform block interiors and black gutters are required.
	for y := b.Min.Y + 3; y < min(b.Min.Y+80, b.Max.Y-3); y++ {
		for x := b.Min.X + 3; x < min(b.Min.X+80, b.Max.X-3); x++ {
			phase := colorAt(x, y)
			if phase < 0 {
				continue
			}
			for pitch := 6; pitch <= 20; pitch++ {
				if !black(x-pitch/2, y) || !black(x+pitch/2, y) || !black(x+pitch+pitch/2, y) || !black(x+2*pitch+pitch/2, y) || !black(x, y-pitch/2) || !black(x, y+pitch/2) {
					continue
				}
				valid := true
				for i := 0; i < 3 && valid; i++ {
					want := (phase + i) % 3
					for _, d := range []image.Point{{0, 0}, {-1, 0}, {1, 0}, {0, -1}, {0, 1}} {
						if colorAt(x+i*pitch+d.X, y+d.Y) != want {
							valid = false
							break
						}
					}
				}
				if valid {
					return StartupBeacon{X: x, Y: y, Pitch: pitch, Phase: phase}, true
				}
			}
		}
	}
	return StartupBeacon{}, false
}
