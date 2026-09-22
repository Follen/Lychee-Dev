// SPDX-License-Identifier: AGPL-3.0-or-later
// BLP2 layout and block decompression adapted from wowdata; see THIRD_PARTY_NOTICES.md.
// Package texture decodes bounded, complete texture levels without filesystem
// state, image encoders, game access or global format registration.
package texture

import (
	"context"
	"encoding/binary"
	"errors"
	"image"
)

var (
	ErrFormat      = errors.New("texture.invalid_format")
	ErrUnsupported = errors.New("texture.unsupported_format")
	ErrLimit       = errors.New("texture.pixel_limit")
	ErrMipmap      = errors.New("texture.mipmap_unavailable")
)

type Description struct {
	Format    string `json:"format"`
	Storage   string `json:"storage"`
	Width     int    `json:"width"`
	Height    int    `json:"height"`
	Mipmap    int    `json:"mipmap"`
	Levels    int    `json:"levels"`
	AlphaBits int    `json:"alphaBits"`
}

// ReadTexture returns owned straight-alpha pixels. All declared byte spans are
// checked before allocation; the selected level must contain exactly the bytes
// its encoding requires. Sparse mip tables retain their original slot numbers.
func ReadTexture(ctx context.Context, raw []byte, mipmap int, maxPixels int64) (*image.NRGBA, Description, error) {
	bad := func(err error) (*image.NRGBA, Description, error) { return nil, Description{}, err }
	if err := ctx.Err(); err != nil {
		return bad(err)
	}
	if maxPixels <= 0 || maxPixels > 64<<20 {
		return bad(ErrLimit)
	}
	if mipmap < 0 || mipmap >= 16 {
		return bad(ErrMipmap)
	}
	if len(raw) < 4 {
		return bad(ErrFormat)
	}
	if string(raw[:4]) != "BLP2" {
		return bad(ErrUnsupported)
	}
	if len(raw) < 148 {
		return bad(ErrFormat)
	}
	if binary.LittleEndian.Uint32(raw[4:8]) != 1 {
		return bad(ErrUnsupported)
	}
	encoding, alpha, alphaMode := raw[8], raw[9], raw[10]
	// Byte 11 carries flags (including 0x11 in Retail assets), not a boolean.
	// Validate mip presence and safety through the offset/size tables below.
	w := uint64(binary.LittleEndian.Uint32(raw[12:16]))
	h := uint64(binary.LittleEndian.Uint32(raw[16:20]))
	if w == 0 || h == 0 {
		return bad(ErrFormat)
	}
	storage, header := "", uint64(148)
	switch encoding {
	case 1:
		if alpha != 0 && alpha != 1 && alpha != 4 && alpha != 8 {
			return bad(ErrUnsupported)
		}
		storage, header = "palette", 1172
	case 2:
		switch {
		case alphaMode == 0 && alpha <= 1:
			storage = "bc1"
		case alphaMode == 1 && (alpha == 0 || alpha == 4 || alpha == 8):
			storage = "bc2"
		case alphaMode == 7 && alpha == 8:
			storage = "bc3"
		default:
			return bad(ErrUnsupported)
		}
	case 3:
		if alpha != 0 && alpha != 8 {
			return bad(ErrUnsupported)
		}
		storage = "bgra8"
	default:
		return bad(ErrUnsupported)
	}
	if uint64(len(raw)) < header {
		return bad(ErrFormat)
	}
	var offsets, sizes [16]uint64
	levels := 0
	for i := range 16 {
		offsets[i] = uint64(binary.LittleEndian.Uint32(raw[20+4*i : 24+4*i]))
		sizes[i] = uint64(binary.LittleEndian.Uint32(raw[84+4*i : 88+4*i]))
		if offsets[i] == 0 && sizes[i] == 0 {
			continue
		}
		if offsets[i] < header || sizes[i] == 0 || offsets[i] > uint64(len(raw)) || sizes[i] > uint64(len(raw))-offsets[i] {
			return bad(ErrFormat)
		}
		for j := 0; j < i; j++ {
			if sizes[j] != 0 && offsets[i] < offsets[j]+sizes[j] && offsets[j] < offsets[i]+sizes[i] {
				return bad(ErrFormat)
			}
		}
		levels++
	}
	if sizes[mipmap] == 0 {
		return bad(ErrMipmap)
	}
	w, h = max(1, w>>mipmap), max(1, h>>mipmap)
	if w > uint64(maxPixels) || h > uint64(maxPixels)/w {
		return bad(ErrLimit)
	}
	pixels := w * h
	want := pixels * 4
	switch storage {
	case "palette":
		want = pixels + (pixels*uint64(alpha)+7)/8
	case "bc1":
		want = ((w + 3) / 4) * ((h + 3) / 4) * 8
	case "bc2", "bc3":
		want = ((w + 3) / 4) * ((h + 3) / 4) * 16
	}
	if sizes[mipmap] != want {
		return bad(ErrFormat)
	}
	data := raw[offsets[mipmap] : offsets[mipmap]+sizes[mipmap]]
	img := image.NewNRGBA(image.Rect(0, 0, int(w), int(h)))
	if storage == "palette" || storage == "bgra8" {
		for i := 0; i < int(pixels); i++ {
			if i&4095 == 0 {
				if err := ctx.Err(); err != nil {
					return bad(err)
				}
			}
			dst := img.Pix[i*4 : i*4+4]
			if storage == "bgra8" {
				copy(dst, []byte{data[i*4+2], data[i*4+1], data[i*4], data[i*4+3]})
				continue
			}
			at := 148 + 4*int(data[i])
			dst[0], dst[1], dst[2], dst[3] = raw[at+2], raw[at+1], raw[at], 255
			if alpha > 0 {
				bit := i * int(alpha)
				value := (data[int(pixels)+bit/8] >> (bit % 8)) & byte((1<<alpha)-1)
				dst[3] = byte(uint16(value) * 255 / uint16((1<<alpha)-1))
			}
		}
	} else {
		step := 16
		if storage == "bc1" {
			step = 8
		}
		at := 0
		for y := 0; y < int(h); y += 4 {
			if err := ctx.Err(); err != nil {
				return bad(err)
			}
			for x := 0; x < int(w); x += 4 {
				paintBlock(img, x, y, data[at:at+step], storage)
				at += step
			}
		}
	}
	if err := ctx.Err(); err != nil {
		return bad(err)
	}
	return img, Description{Format: "BLP2", Storage: storage, Width: int(w), Height: int(h), Mipmap: mipmap, Levels: levels, AlphaBits: int(alpha)}, nil
}

func paintBlock(dst *image.NRGBA, x, y int, data []byte, storage string) {
	colorsAt := 0
	if storage != "bc1" {
		colorsAt = 8
	}
	first := binary.LittleEndian.Uint16(data[colorsAt:])
	second := binary.LittleEndian.Uint16(data[colorsAt+2:])
	colors := [4][4]byte{expandColor(first), expandColor(second), {0, 0, 0, 255}, {0, 0, 0, 255}}
	if storage == "bc1" && first <= second {
		for c := range 3 {
			colors[2][c] = byte((uint16(colors[0][c]) + uint16(colors[1][c])) / 2)
		}
		colors[3] = [4]byte{}
	} else {
		for c := range 3 {
			colors[2][c] = byte((2*uint16(colors[0][c]) + uint16(colors[1][c])) / 3)
			colors[3][c] = byte((uint16(colors[0][c]) + 2*uint16(colors[1][c])) / 3)
		}
	}
	indices := binary.LittleEndian.Uint32(data[colorsAt+4:])
	var opacity [8]byte
	var alphaIndices uint64
	if storage == "bc3" {
		opacity[0], opacity[1] = data[0], data[1]
		divisor := 7
		if opacity[0] <= opacity[1] {
			divisor = 5
			opacity[6], opacity[7] = 0, 255
		}
		for i := 1; i < divisor; i++ {
			opacity[i+1] = byte(((divisor-i)*int(opacity[0]) + i*int(opacity[1])) / divisor)
		}
		for i := range 6 {
			alphaIndices |= uint64(data[2+i]) << (8 * i)
		}
	}
	for row := range 4 {
		for column := range 4 {
			if x+column >= dst.Rect.Dx() || y+row >= dst.Rect.Dy() {
				continue
			}
			i := row*4 + column
			color := colors[(indices>>(2*i))&3]
			switch storage {
			case "bc2":
				color[3] = ((data[i/2] >> (4 * (i % 2))) & 15) * 17
			case "bc3":
				color[3] = opacity[(alphaIndices>>(3*i))&7]
			}
			at := (y+row)*dst.Stride + (x+column)*4
			copy(dst.Pix[at:at+4], color[:])
		}
	}
}

func expandColor(packed uint16) [4]byte {
	r, g, b := (packed>>11)&31, (packed>>5)&63, packed&31
	return [4]byte{byte(r<<3 | r>>2), byte(g<<2 | g>>4), byte(b<<3 | b>>2), 255}
}
