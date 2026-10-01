package bridge

import (
	"errors"
	"image"
)

const InputSignalCapability = "lycheedev.input.hybrid.v2"

type InputSignal struct {
	State     string
	Heartbeat bool
}

// DecodeInputSignal decodes exactly three 2x2 physical-pixel cells. Every pixel
// must independently classify as the same canonical color in its cell.
// This carries keyboard state only, never runtime identity or input authority.
func DecodeInputSignal(im *image.NRGBA) (InputSignal, error) {
	if im == nil || im.Bounds().Dx() != 6 || im.Bounds().Dy() != 2 {
		return InputSignal{}, errors.New("bridge.input_signal_extent")
	}
	classify := func(x, y int) byte {
		c := im.NRGBAAt(x, y)
		if c.A < 250 {
			return '?'
		}
		low := func(v uint8) bool { return v <= 31 }
		high := func(v uint8) bool { return v >= 224 }
		switch {
		case low(c.R) && low(c.G) && low(c.B):
			return 'k'
		case high(c.R) && high(c.G) && high(c.B):
			return 'w'
		case high(c.R) && low(c.G) && low(c.B):
			return 'r'
		case low(c.R) && high(c.G) && low(c.B):
			return 'g'
		case low(c.R) && low(c.G) && high(c.B):
			return 'b'
		}
		return '?'
	}
	var cells [3]byte
	for i := range cells {
		x, y := im.Bounds().Min.X+i*2, im.Bounds().Min.Y
		cells[i] = classify(x, y)
		if cells[i] == '?' {
			return InputSignal{}, errors.New("bridge.input_signal_color")
		}
		for yy := y; yy < y+2; yy++ {
			for xx := x; xx < x+2; xx++ {
				if classify(xx, yy) != cells[i] {
					return InputSignal{}, errors.New("bridge.input_signal_mixed")
				}
			}
		}
	}
	if cells[2] != 'k' && cells[2] != 'w' {
		return InputSignal{}, errors.New("bridge.input_signal_heartbeat")
	}
	state := ""
	switch string(cells[:2]) {
	case "gw":
		state = "ready"
	case "rb":
		state = "input_keyboard_focus"
	case "br":
		state = "input_combat_lockdown"
	case "wk":
		state = "unknown"
	default:
		return InputSignal{}, errors.New("bridge.input_signal_code")
	}
	return InputSignal{State: state, Heartbeat: cells[2] == 'w'}, nil
}
