package desktop

import "image"

// ReadReloadPatch recognizes one colored square with a black surround. This is
// a frame observation only; callers must check temporal phases and fresh ticks.
func ReadReloadPatch(p *image.NRGBA) (StartupBeacon, bool) {
	if p == nil {
		return StartupBeacon{}, false
	}
	bounds := p.Bounds()
	phase := func(x, y int) int {
		if !image.Pt(x, y).In(bounds) {
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
		if !image.Pt(x, y).In(bounds) {
			return false
		}
		c := p.NRGBAAt(x, y)
		return c.R < 50 && c.G < 50 && c.B < 50
	}
	for y := bounds.Min.Y + 1; y < min(bounds.Min.Y+80, bounds.Max.Y-4); y++ {
		for x := bounds.Min.X + 1; x < min(bounds.Min.X+80, bounds.Max.X-4); x++ {
			color := phase(x, y)
			if color < 0 {
				continue
			}
			// Find the top-left corner, then validate the entire square and
			// its surround. Symmetric center samples miss even-sized patches
			// whose black border becomes one pixel under client UI scaling.
			if !black(x-1, y) || !black(x, y-1) {
				continue
			}
			size := 0
			for size <= 12 && phase(x+size, y) == color {
				size++
			}
			if size < 4 || size > 12 {
				continue
			}
			valid := true
			for dy := -1; dy <= size && valid; dy++ {
				for dx := -1; dx <= size; dx++ {
					if dx == -1 || dy == -1 || dx == size || dy == size {
						valid = black(x+dx, y+dy)
					} else {
						valid = phase(x+dx, y+dy) == color
					}
					if !valid {
						break
					}
				}
			}
			if valid {
				return StartupBeacon{X: x + size/2, Y: y + size/2, Pitch: size + 2, Phase: color}, true
			}
		}
	}
	return StartupBeacon{}, false
}
