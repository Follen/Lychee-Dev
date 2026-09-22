package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"image/png"
	"strings"

	"github.com/HugoSmits86/nativewebp"
	"github.com/follenfang/lycheedev/internal/records/texture"
)

var ErrExportOptions = errors.New("records.export_options")
var ErrImageOutputLimit = errors.New("records.image_output_limit")

type ImageExport struct {
	Texture      texture.Description `json:"texture"`
	Channels     string              `json:"channels"`
	PixelsSHA256 string              `json:"pixelsSHA256"`
}

func normalizeAssetExport(r AssetExportRequest) (AssetExportRequest, error) {
	if r.Encoding == "" {
		r.Encoding = "raw"
	}
	if r.Encoding == "raw" {
		if r.Mipmap != 0 || r.Channels != "" || r.MaxPixels != 0 {
			return r, ErrExportOptions
		}
		return r, nil
	}
	if r.Encoding != "png" && r.Encoding != "webp" || r.Mipmap < 0 || r.Mipmap > 15 {
		return r, ErrExportOptions
	}
	if r.MaxPixels == 0 {
		r.MaxPixels = 16 << 20
	}
	if r.MaxPixels < 1 || r.MaxPixels > 64<<20 {
		return r, ErrExportOptions
	}
	if r.Channels == "" {
		r.Channels = "rgba"
	}
	var channels uint8
	for _, c := range r.Channels {
		i := strings.IndexRune("rgba", c)
		if i < 0 || channels&(1<<i) != 0 {
			return r, ErrExportOptions
		}
		channels |= 1 << i
	}
	r.Channels = ""
	for i, c := range "rgba" {
		if channels&(1<<i) != 0 {
			r.Channels += string(c)
		}
	}
	return r, nil
}

func encodeAssetImage(ctx context.Context, raw []byte, r AssetExportRequest) ([]byte, *ImageExport, error) {
	img, description, err := texture.ReadTexture(ctx, raw, r.Mipmap, r.MaxPixels)
	if err != nil {
		return nil, nil, err
	}
	if r.Encoding == "webp" && (description.Width > 16384 || description.Height > 16384) {
		return nil, nil, texture.ErrLimit
	}
	for i := 0; i < len(img.Pix); i += 4 {
		if i&16383 == 0 {
			if err := ctx.Err(); err != nil {
				return nil, nil, err
			}
		}
		if len(r.Channels) == 1 {
			// Single-channel inspection is an opaque grayscale image, including alpha.
			v := img.Pix[i+strings.IndexByte("rgba", r.Channels[0])]
			img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = v, v, v, 255
		} else {
			for c, name := range "rgba" {
				if !strings.ContainsRune(r.Channels, name) {
					img.Pix[i+c] = 0
					if c == 3 {
						img.Pix[i+c] = 255
					}
				}
			}
		}
	}
	digest := sha256.Sum256(img.Pix)
	info := &ImageExport{Texture: description, Channels: r.Channels, PixelsSHA256: hex.EncodeToString(digest[:])}
	output := &imageOutput{ctx: ctx, limit: r.File.ContentBytes}
	switch r.Encoding {
	case "png":
		err = png.Encode(output, img)
	case "webp":
		err = nativewebp.Encode(output, img, &nativewebp.Options{CompressionLevel: nativewebp.DefaultCompression})
	}
	// nativewebp 1.3.0 does not propagate every Writer error. Retain the first
	// write failure independently, and never publish an incomplete bitstream.
	if output.err != nil {
		return nil, nil, output.err
	}
	if err != nil {
		return nil, nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	return output.buffer.Bytes(), info, nil
}

type imageOutput struct {
	ctx    context.Context
	buffer bytes.Buffer
	limit  int64
	err    error
}

func (w *imageOutput) Write(raw []byte) (int, error) {
	if w.err == nil {
		w.err = w.ctx.Err()
	}
	if w.err == nil && int64(len(raw)) > w.limit-int64(w.buffer.Len()) {
		w.err = ErrImageOutputLimit
	}
	if w.err != nil {
		return 0, w.err
	}
	return w.buffer.Write(raw)
}
