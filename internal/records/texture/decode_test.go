package texture_test

import (
	"context"
	"encoding/binary"
	"errors"
	"image"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/texture"
)

type mipFixture struct {
	offset int
	data   []byte
	size   int
}

// makeBLP writes only the BLP2 container. Pixel bytes and palette entries are
// supplied by each fixture so the expected pixels remain independently
// hand-calculated.
func makeBLP(compression, alphaDepth, alphaEncoding byte, width, height uint32, palette []byte, levels map[int]mipFixture, sourceLen int) []byte {
	headerEnd := 148
	if compression == 1 {
		headerEnd += 1024
	}

	if sourceLen == 0 {
		sourceLen = headerEnd
		for _, level := range levels {
			declared := level.size
			if declared < 0 {
				declared = len(level.data)
			}
			if end := level.offset + declared; end > sourceLen {
				sourceLen = end
			}
		}
	}
	raw := make([]byte, sourceLen)
	if len(raw) >= 4 {
		copy(raw, []byte("BLP2"))
	}
	if len(raw) >= 8 {
		binary.LittleEndian.PutUint32(raw[4:8], 1)
	}
	if len(raw) > 8 {
		raw[8] = compression
	}
	if len(raw) > 9 {
		raw[9] = alphaDepth
	}
	if len(raw) > 10 {
		raw[10] = alphaEncoding
	}
	if len(raw) > 11 {
		for level := range levels {
			if level > 0 {
				raw[11] = 1
				break
			}
		}
	}
	if len(raw) >= 20 {
		binary.LittleEndian.PutUint32(raw[12:16], width)
		binary.LittleEndian.PutUint32(raw[16:20], height)
	}
	if compression == 1 && len(palette) == 1024 && len(raw) >= 1172 {
		copy(raw[148:1172], palette)
	}
	for level, fixture := range levels {
		if level < 0 || level >= 16 || len(raw) < 148 {
			continue
		}
		declared := fixture.size
		if declared < 0 {
			declared = len(fixture.data)
		}
		binary.LittleEndian.PutUint32(raw[20+level*4:24+level*4], uint32(fixture.offset))
		binary.LittleEndian.PutUint32(raw[84+level*4:88+level*4], uint32(declared))
		if fixture.offset >= 0 && fixture.offset < len(raw) {
			copy(raw[fixture.offset:], fixture.data)
		}
	}
	return raw
}

func oneLevelBLP(compression, alphaDepth, alphaEncoding byte, width, height uint32, palette []byte, payload []byte) []byte {
	headerEnd := 148
	if compression == 1 {
		headerEnd += 1024
	}
	return makeBLP(compression, alphaDepth, alphaEncoding, width, height, palette,
		map[int]mipFixture{0: {offset: headerEnd + 4, data: payload, size: -1}}, 0)
}

func paletteTable() []byte {
	palette := make([]byte, 1024)
	// Palette entries are B, G, R, reserved. The reserved byte must not become
	// the output alpha when an appended alpha plane is present.
	entries := [][4]byte{
		{30, 20, 10, 7},
		{60, 50, 40, 77},
		{90, 80, 70, 123},
		{120, 110, 100, 201},
	}
	for index, entry := range entries {
		copy(palette[index*4:index*4+4], entry[:])
	}
	return palette
}

func palettePayload(depth byte) []byte {
	payload := []byte{0, 1, 2, 3}
	switch depth {
	case 1:
		payload = append(payload, 0x05) // pixels 0..3: 1, 0, 1, 0
	case 4:
		payload = append(payload, 0x21, 0xfa) // low nibble first
	case 8:
		payload = append(payload, 0, 51, 128, 255)
	}
	return payload
}

func expectPixels(t *testing.T, got *image.NRGBA, want [][][4]uint8) {
	t.Helper()
	if got == nil {
		t.Fatal("ReadTexture returned nil pixels")
	}
	if got.Bounds().Dx() != len(want[0]) || got.Bounds().Dy() != len(want) {
		t.Fatalf("bounds = %v, want %dx%d", got.Bounds(), len(want[0]), len(want))
	}
	for y, row := range want {
		for x, expected := range row {
			if actual := got.NRGBAAt(x, y); [4]uint8{actual.R, actual.G, actual.B, actual.A} != expected {
				t.Fatalf("pixel (%d,%d) = %#v, want %#v", x, y, actual, expected)
			}
		}
	}
}

func expectDescription(t *testing.T, got texture.Description, storage string, width, height, mipmap, levels, alphaBits int) {
	t.Helper()
	if got.Format != "BLP2" || got.Storage != storage || got.Width != width || got.Height != height || got.Mipmap != mipmap || got.Levels != levels || got.AlphaBits != alphaBits {
		t.Fatalf("description = %#v, want BLP2/%s %dx%d mip=%d levels=%d alpha=%d", got, storage, width, height, mipmap, levels, alphaBits)
	}
}

func expectError(t *testing.T, raw []byte, mipmap int, maxPixels int64, want error) {
	t.Helper()
	pixels, description, err := texture.ReadTexture(context.Background(), raw, mipmap, maxPixels)
	if !errors.Is(err, want) {
		t.Fatalf("ReadTexture error = %v, want %v", err, want)
	}
	if pixels != nil || description != (texture.Description{}) {
		t.Fatalf("failed ReadTexture returned pixels=%v description=%#v", pixels, description)
	}
}

func TestReadTexturePaletteAlphaPlanes(t *testing.T) {
	wantAlpha := map[byte][]uint8{
		0: {255, 255, 255, 255},
		1: {255, 0, 255, 0},
		4: {17, 34, 170, 255},
		8: {0, 51, 128, 255},
	}
	for _, depth := range []byte{0, 1, 4, 8} {
		t.Run("alpha-"+string(rune('0'+depth)), func(t *testing.T) {
			raw := oneLevelBLP(1, depth, 0, 2, 2, paletteTable(), palettePayload(depth))
			pixels, description, err := texture.ReadTexture(context.Background(), raw, 0, 64)
			if err != nil {
				t.Fatal(err)
			}
			expectDescription(t, description, "palette", 2, 2, 0, 1, int(depth))
			expectPixels(t, pixels, [][][4]uint8{
				{{10, 20, 30, wantAlpha[depth][0]}, {40, 50, 60, wantAlpha[depth][1]}},
				{{70, 80, 90, wantAlpha[depth][2]}, {100, 110, 120, wantAlpha[depth][3]}},
			})
		})
	}
}

func TestReadTextureBC1InterpolationAndTransparency(t *testing.T) {
	// Green and black make the 2/3 and 1/3 RGB565 interpolants exact after
	// expansion to eight bits. Selectors are in row-major, low-two-bit order.
	colorBlock := func(color0, color1 uint16, selectors uint32) []byte {
		block := make([]byte, 8)
		binary.LittleEndian.PutUint16(block[0:2], color0)
		binary.LittleEndian.PutUint16(block[2:4], color1)
		binary.LittleEndian.PutUint32(block[4:8], selectors)
		return block
	}
	tests := []struct {
		name    string
		payload []byte
		want    [][4]uint8
	}{
		{
			name:    "opaque-four-color-mode",
			payload: colorBlock(0x07e0, 0x0000, 0xe4e4e4e4),
			want:    [][4]uint8{{0, 255, 0, 255}, {0, 0, 0, 255}, {0, 170, 0, 255}, {0, 85, 0, 255}},
		},
		{
			name:    "transparent-three-color-mode",
			payload: colorBlock(0x0000, 0xffff, 0xe4e4e4e4),
			want:    [][4]uint8{{0, 0, 0, 255}, {255, 255, 255, 255}, {127, 127, 127, 255}, {0, 0, 0, 0}},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			raw := oneLevelBLP(2, 0, 0, 4, 1, nil, test.payload)
			pixels, description, err := texture.ReadTexture(context.Background(), raw, 0, 64)
			if err != nil {
				t.Fatal(err)
			}
			expectDescription(t, description, "bc1", 4, 1, 0, 1, 0)
			expectPixels(t, pixels, [][][4]uint8{test.want})
		})
	}
}

func TestReadTextureBC2NibbleAlpha(t *testing.T) {
	payload := make([]byte, 16)
	payload[0] = 0x50 // alpha nibbles 0, 5
	payload[1] = 0xfa // alpha nibbles 10, 15
	// Remaining alpha bytes are zero. The color block is solid green.
	binary.LittleEndian.PutUint16(payload[8:10], 0x07e0)
	binary.LittleEndian.PutUint16(payload[10:12], 0x0000)
	pixels, description, err := texture.ReadTexture(context.Background(), oneLevelBLP(2, 4, 1, 4, 1, nil, payload), 0, 64)
	if err != nil {
		t.Fatal(err)
	}
	expectDescription(t, description, "bc2", 4, 1, 0, 1, 4)
	expectPixels(t, pixels, [][][4]uint8{{{0, 255, 0, 0}, {0, 255, 0, 85}, {0, 255, 0, 170}, {0, 255, 0, 255}}})
}

func packBC3Alpha(indices ...uint8) []byte {
	var packed uint64
	for index, value := range indices {
		packed |= uint64(value&7) << (3 * index)
	}
	result := make([]byte, 8)
	binary.LittleEndian.PutUint64(result, packed)
	return result
}

func bc3Payload(a0, a1 byte, indices ...uint8) []byte {
	payload := make([]byte, 16)
	payload[0], payload[1] = a0, a1
	copy(payload[2:8], packBC3Alpha(indices...)[:6])
	binary.LittleEndian.PutUint16(payload[8:10], 0x07e0)
	binary.LittleEndian.PutUint16(payload[10:12], 0x0000)
	return payload
}

func TestReadTextureBC3AlphaInterpolationBranches(t *testing.T) {
	tests := []struct {
		name      string
		payload   []byte
		wantAlpha []uint8
	}{
		{
			name:      "a0-greater-than-a1",
			payload:   bc3Payload(255, 0, 0, 1, 2, 3, 4, 5, 6, 7),
			wantAlpha: []uint8{255, 0, 218, 182, 145, 109, 72, 36},
		},
		{
			name:      "a0-less-than-a1",
			payload:   bc3Payload(0, 100, 0, 1, 2, 3, 4, 5, 6, 7),
			wantAlpha: []uint8{0, 100, 20, 40, 60, 80, 0, 255},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			raw := oneLevelBLP(2, 8, 7, 4, 2, nil, test.payload)
			pixels, description, err := texture.ReadTexture(context.Background(), raw, 0, 64)
			if err != nil {
				t.Fatal(err)
			}
			expectDescription(t, description, "bc3", 4, 2, 0, 1, 8)
			want := make([][][4]uint8, 2)
			for index, alpha := range test.wantAlpha {
				want[index/4] = append(want[index/4], [4]uint8{0, 255, 0, alpha})
			}
			expectPixels(t, pixels, want)
		})
	}
}

func TestReadTextureBGRAAndOddBlockEdges(t *testing.T) {
	t.Run("raw-bgra-straight-alpha", func(t *testing.T) {
		payload := []byte{3, 2, 1, 4, 30, 20, 10, 128}
		pixels, description, err := texture.ReadTexture(context.Background(), oneLevelBLP(3, 8, 0, 2, 1, nil, payload), 0, 64)
		if err != nil {
			t.Fatal(err)
		}
		expectDescription(t, description, "bgra8", 2, 1, 0, 1, 8)
		expectPixels(t, pixels, [][][4]uint8{{{1, 2, 3, 4}, {10, 20, 30, 128}}})
	})

	t.Run("bc1-crops-odd-edges", func(t *testing.T) {
		payload := make([]byte, 8)
		binary.LittleEndian.PutUint16(payload[0:2], 0x07e0)
		binary.LittleEndian.PutUint16(payload[2:4], 0x0000)
		binary.LittleEndian.PutUint32(payload[4:8], 0xe4e4e4e4)
		pixels, description, err := texture.ReadTexture(context.Background(), oneLevelBLP(2, 0, 0, 3, 2, nil, payload), 0, 64)
		if err != nil {
			t.Fatal(err)
		}
		expectDescription(t, description, "bc1", 3, 2, 0, 1, 0)
		expectPixels(t, pixels, [][][4]uint8{
			{{0, 255, 0, 255}, {0, 0, 0, 255}, {0, 170, 0, 255}},
			{{0, 255, 0, 255}, {0, 0, 0, 255}, {0, 170, 0, 255}},
		})
	})
}

func TestReadTextureMipmapSlotSelectionAndGaps(t *testing.T) {
	base := 148
	mip0 := []byte{3, 2, 1, 255, 30, 20, 10, 255, 90, 80, 70, 255, 120, 110, 100, 255, 0, 0, 0, 255, 1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255}
	mip2 := []byte{9, 8, 7, 6}
	raw := makeBLP(3, 8, 0, 4, 2, nil, map[int]mipFixture{
		0: {offset: base + 8, data: mip0, size: -1},
		2: {offset: base + 64, data: mip2, size: -1},
	}, 0)

	pixels, description, err := texture.ReadTexture(context.Background(), raw, 2, 64)
	if err != nil {
		t.Fatal(err)
	}
	expectDescription(t, description, "bgra8", 1, 1, 2, 2, 8)
	expectPixels(t, pixels, [][][4]uint8{{{7, 8, 9, 6}}})
	expectError(t, raw, 1, 64, texture.ErrMipmap)
}

func TestReadTextureRejectsMalformedContainersAndUnsupportedFormats(t *testing.T) {
	valid := oneLevelBLP(3, 8, 0, 1, 1, nil, []byte{3, 2, 1, 255})

	tests := []struct {
		name string
		raw  []byte
		want error
	}{
		{name: "short-header", raw: []byte("BLP2"), want: texture.ErrFormat},
		{name: "blp1-unsupported", raw: append([]byte("BLP1"), make([]byte, 144)...), want: texture.ErrUnsupported},
		{name: "jpeg-unsupported", raw: append([]byte{0xff, 0xd8, 0xff, 0xe0}, make([]byte, 144)...), want: texture.ErrUnsupported},
		{name: "unknown-compression", raw: func() []byte { raw := append([]byte(nil), valid...); raw[8] = 9; return raw }(), want: texture.ErrUnsupported},
		{name: "zero-dimensions", raw: func() []byte {
			raw := append([]byte(nil), valid...)
			binary.LittleEndian.PutUint32(raw[12:16], 0)
			return raw
		}(), want: texture.ErrFormat},
		{name: "truncated-payload", raw: func() []byte {
			raw := append([]byte(nil), valid[:len(valid)-1]...)
			binary.LittleEndian.PutUint32(raw[84:88], 4)
			return raw
		}(), want: texture.ErrFormat},
		{name: "wrong-payload-size", raw: oneLevelBLP(3, 8, 0, 1, 1, nil, []byte{3, 2, 1}), want: texture.ErrFormat},
		{name: "span-inside-header", raw: makeBLP(3, 8, 0, 1, 1, nil, map[int]mipFixture{0: {offset: 140, data: []byte{3, 2, 1, 255}, size: -1}}, 148), want: texture.ErrFormat},
		{name: "overlapping-mips", raw: makeBLP(3, 8, 0, 2, 1, nil, map[int]mipFixture{
			0: {offset: 156, data: make([]byte, 8), size: -1},
			1: {offset: 160, data: make([]byte, 4), size: -1},
		}, 164), want: texture.ErrFormat},
		{name: "palette-missing", raw: makeBLP(1, 0, 0, 1, 1, nil, nil, 148), want: texture.ErrFormat},
		{name: "palette-span-in-palette", raw: makeBLP(1, 0, 0, 1, 1, paletteTable(), map[int]mipFixture{0: {offset: 148, data: []byte{0}, size: -1}}, 1173), want: texture.ErrFormat},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			expectError(t, test.raw, 0, 64, test.want)
		})
	}
}

func TestReadTextureLimitsAndMipmapArguments(t *testing.T) {
	valid := oneLevelBLP(3, 8, 0, 2, 2, nil, []byte{
		0, 0, 0, 255, 0, 0, 0, 255,
		0, 0, 0, 255, 0, 0, 0, 255,
	})
	expectError(t, valid, 0, 0, texture.ErrLimit)
	expectError(t, valid, 0, 64*1024*1024+1, texture.ErrLimit)
	expectError(t, valid, 0, 3, texture.ErrLimit)
	expectError(t, valid, -1, 64, texture.ErrMipmap)
	expectError(t, valid, 16, 64, texture.ErrMipmap)

	large := makeBLP(3, 8, 0, 65, 65, nil, map[int]mipFixture{
		0: {offset: 152, data: make([]byte, 65*65*4), size: -1},
	}, 0)
	expectError(t, large, 0, 4096, texture.ErrLimit)
}

func TestReadTextureChecksContextAndReturnsNoPartialResult(t *testing.T) {
	raw := oneLevelBLP(3, 8, 0, 1, 1, nil, []byte{3, 2, 1, 255})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	pixels, description, err := texture.ReadTexture(ctx, raw, 0, 64)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("ReadTexture error = %v, want context.Canceled", err)
	}
	if pixels != nil || description != (texture.Description{}) {
		t.Fatalf("cancelled ReadTexture returned pixels=%v description=%#v", pixels, description)
	}
}

func FuzzReadTextureBounded(f *testing.F) {
	seeds := [][]byte{
		oneLevelBLP(3, 8, 0, 1, 1, nil, []byte{3, 2, 1, 255}),
		oneLevelBLP(2, 0, 0, 4, 4, nil, []byte{0, 0, 0, 0, 0, 0, 0, 0}),
		[]byte("BLP1"),
		[]byte("not a texture"),
	}
	for _, seed := range seeds {
		f.Add(seed, uint8(0), uint16(4096))
	}
	f.Fuzz(func(t *testing.T, raw []byte, mipmap uint8, maxPixels uint16) {
		pixels, description, err := texture.ReadTexture(context.Background(), raw, int(mipmap%16), int64(maxPixels%4096+1))
		if err != nil {
			if pixels != nil || description != (texture.Description{}) {
				t.Fatalf("error %v returned pixels=%v description=%#v", err, pixels, description)
			}
			return
		}
		if pixels == nil {
			t.Fatal("successful ReadTexture returned nil pixels")
		}
		if pixels.Bounds().Dx() <= 0 || pixels.Bounds().Dy() <= 0 || int64(pixels.Bounds().Dx())*int64(pixels.Bounds().Dy()) > 4096 {
			t.Fatalf("successful result has unbounded dimensions: %v", pixels.Bounds())
		}
		if description.Width != pixels.Bounds().Dx() || description.Height != pixels.Bounds().Dy() {
			t.Fatalf("description %#v does not match pixels %v", description, pixels.Bounds())
		}
	})
}
