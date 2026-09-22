package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"image"
	"image/color"
	"image/png"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/texture"
	"golang.org/x/image/webp"
)

func imageFixture() ([]byte, []byte) {
	rgba := []byte{255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 20, 40, 60, 72}
	raw := make([]byte, 148)
	copy(raw, "BLP2")
	binary.LittleEndian.PutUint32(raw[4:], 1)
	raw[8], raw[9] = 3, 8
	binary.LittleEndian.PutUint32(raw[12:], 2)
	binary.LittleEndian.PutUint32(raw[16:], 2)
	binary.LittleEndian.PutUint32(raw[20:], 148)
	binary.LittleEndian.PutUint32(raw[84:], uint32(len(rgba)))
	for i := 0; i < len(rgba); i += 4 {
		raw = append(raw, rgba[i+2], rgba[i+1], rgba[i], rgba[i+3])
	}
	return raw, rgba
}

func TestImageExportRoundTripPixels(t *testing.T) {
	raw, rgba := imageFixture()
	for _, encoding := range []string{"png", "webp"} {
		for _, channels := range []string{"rgba", "rgb", "r", "g", "b", "a", "ra", "bg"} {
			t.Run(encoding+"/"+channels, func(t *testing.T) {
				r, err := normalizeAssetExport(AssetExportRequest{Encoding: encoding, Channels: channels, File: FileQuery{ContentBytes: 1 << 20}})
				if err != nil {
					t.Fatal(err)
				}
				encoded, info, err := encodeAssetImage(context.Background(), raw, r)
				if err != nil {
					t.Fatal(err)
				}
				var decoded image.Image
				if encoding == "png" {
					decoded, err = png.Decode(bytes.NewReader(encoded))
				} else {
					decoded, err = webp.Decode(bytes.NewReader(encoded))
				}
				if err != nil {
					t.Fatal(err)
				}
				if decoded.Bounds() != image.Rect(0, 0, 2, 2) || info.Texture.Storage != "bgra8" {
					t.Fatal(decoded.Bounds(), info)
				}
				want := bytes.Clone(rgba)
				for i := 0; i < len(want); i += 4 {
					if len(channels) == 1 {
						position := map[string]int{"r": 0, "g": 1, "b": 2, "a": 3}[channels]
						v := rgba[i+position]
						copy(want[i:], []byte{v, v, v, 255})
					} else {
						for c, name := range "rgba" {
							found := false
							for _, selected := range channels {
								found = found || selected == name
							}
							if !found {
								want[i+c] = 0
								if c == 3 {
									want[i+c] = 255
								}
							}
						}
					}
				}
				for i := 0; i < len(want)/4; i++ {
					actual := color.NRGBAModel.Convert(decoded.At(i%2, i/2)).(color.NRGBA)
					if !bytes.Equal([]byte{actual.R, actual.G, actual.B, actual.A}, want[i*4:i*4+4]) {
						t.Fatalf("pixel %d: %+v want %v", i, actual, want[i*4:i*4+4])
					}
				}
				digest := sha256.Sum256(want)
				if info.PixelsSHA256 != hex.EncodeToString(digest[:]) {
					t.Fatal(info)
				}
			})
		}
	}
}

func TestImageExportRejectsBudgetAndCancellation(t *testing.T) {
	raw, _ := imageFixture()
	for _, encoding := range []string{"png", "webp"} {
		r, err := normalizeAssetExport(AssetExportRequest{Encoding: encoding, File: FileQuery{ContentBytes: 1}})
		if err != nil {
			t.Fatal(err)
		}
		encoded, info, err := encodeAssetImage(context.Background(), raw, r)
		if !errors.Is(err, ErrImageOutputLimit) || encoded != nil || info != nil {
			t.Fatal(encoding, encoded, info, err)
		}
		r.File.ContentBytes = 1 << 20
		r.MaxPixels = 3
		if _, _, err := encodeAssetImage(context.Background(), raw, r); !errors.Is(err, texture.ErrLimit) {
			t.Fatal(err)
		}
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, _, err := encodeAssetImage(ctx, raw, r); !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	}
}

func TestImageExportOptions(t *testing.T) {
	for _, r := range []AssetExportRequest{
		{Encoding: "jpeg"}, {Encoding: "png", Channels: "rr"}, {Encoding: "png", Channels: "R"}, {Encoding: "png", Channels: "red"},
		{Encoding: "png", Mipmap: -1}, {Encoding: "webp", Mipmap: 16}, {Encoding: "png", MaxPixels: -1}, {Encoding: "webp", MaxPixels: 64<<20 + 1},
		{Encoding: "raw", Mipmap: 1}, {Encoding: "raw", Channels: "rgba"}, {Encoding: "raw", MaxPixels: 1},
	} {
		if _, err := normalizeAssetExport(r); !errors.Is(err, ErrExportOptions) {
			t.Fatalf("%+v: %v", r, err)
		}
		if _, err := ExportAsset(context.Background(), "must-not-open", "unused", r); !errors.Is(err, ErrExportOptions) {
			t.Fatalf("opened path before validation: %v", err)
		}
	}
	r, err := normalizeAssetExport(AssetExportRequest{Encoding: "png", Channels: "abgr"})
	if err != nil || r.Channels != "rgba" || r.MaxPixels != 16<<20 {
		t.Fatal(r, err)
	}
}

func TestImageOutputRetainsFirstFailure(t *testing.T) {
	w := &imageOutput{ctx: context.Background(), limit: 2}
	if n, err := w.Write([]byte{1, 2, 3}); n != 0 || !errors.Is(err, ErrImageOutputLimit) {
		t.Fatal(n, err)
	}
	if n, err := w.Write([]byte{1}); n != 0 || !errors.Is(err, ErrImageOutputLimit) || w.buffer.Len() != 0 {
		t.Fatal(n, err)
	}
}
