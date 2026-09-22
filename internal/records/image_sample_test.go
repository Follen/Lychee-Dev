package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/texture"
	"golang.org/x/image/webp"
)

// Optional read-only validation of separately exported real assets. CI uses
// synthetic fixtures; game content is neither downloaded nor bundled here.
func TestImageExportRealSample(t *testing.T) {
	directory := os.Getenv("LYCHEEDEV_IMAGE_SAMPLE")
	if directory == "" {
		t.Skip("set LYCHEEDEV_IMAGE_SAMPLE to a directory containing icon.blp/png/webp")
	}
	raw, err := os.ReadFile(filepath.Join(directory, "icon.blp"))
	if err != nil {
		t.Fatal(err)
	}
	want, description, err := texture.ReadTexture(context.Background(), raw, 0, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	for _, encoding := range []string{"png", "webp"} {
		encoded, err := os.ReadFile(filepath.Join(directory, "icon."+encoding))
		if err != nil {
			t.Fatal(err)
		}
		var got image.Image
		if encoding == "png" {
			got, err = png.Decode(bytes.NewReader(encoded))
		} else {
			got, err = webp.Decode(bytes.NewReader(encoded))
		}
		if err != nil {
			t.Fatal(err)
		}
		if got.Bounds() != want.Bounds() {
			t.Fatalf("%s: bounds %v != %v", encoding, got.Bounds(), want.Bounds())
		}
		for y := range want.Bounds().Dy() {
			for x := range want.Bounds().Dx() {
				actual := color.NRGBAModel.Convert(got.At(x, y)).(color.NRGBA)
				if actual != want.NRGBAAt(x, y) {
					t.Fatalf("%s pixel (%d,%d): %v != %v", encoding, x, y, actual, want.NRGBAAt(x, y))
				}
			}
		}
		t.Logf("%s bytes=%d sha256=%x", encoding, len(encoded), sha256.Sum256(encoded))
	}
	t.Logf("texture=%+v rgbaSHA256=%x sourceSHA256=%x", description, sha256.Sum256(want.Pix), sha256.Sum256(raw))
}
