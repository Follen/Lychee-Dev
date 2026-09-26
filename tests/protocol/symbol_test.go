package protocol_test

import (
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
)

type symbolDrawing struct {
	Width, Height int
	Runs          []string
}

type symbolRun struct{ x, y, w, h int }

func parseSymbolRuns(t *testing.T, encoded []string) []symbolRun {
	t.Helper()
	runs := make([]symbolRun, 0, len(encoded))
	for _, run := range encoded {
		var parsed symbolRun
		if count, err := fmt.Sscanf(run, "%d,%d,%d,%d", &parsed.x, &parsed.y, &parsed.w, &parsed.h); err != nil || count != 4 {
			t.Fatalf("invalid run %q", run)
		}
		runs = append(runs, parsed)
	}
	return runs
}

// Every dark run shares one module size and keeps the 4-module quiet zone that
// the receipt card reserves on each edge of every symbol.
func symbolRunInvalid(run symbolRun, module, width, height int) bool {
	quiet := 4 * module
	return run.h != module || run.w < module || run.x < quiet || run.y < quiet ||
		run.x+run.w > width-quiet || run.y+run.h > height-quiet
}

func TestLuaReceiptDrawingDecodedByHost(t *testing.T) {
	for _, payload := range []string{
		`{"schema":"lycheedev.signal.v1","kind":"loaded","character":"奶骑","realm":"世界","sequence":1}`,
		"012345678901234567890123456789",
		"ALPHANUMERIC +%/TEST",
		strings.Repeat("x", 2048),
	} {
		t.Run(string(rune('A'+len(payload)%26)), func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "receipt.txt")
			if err := os.WriteFile(path, []byte(payload), 0600); err != nil {
				t.Fatal(err)
			}
			output, err := exec.Command(luaRuntime(t), "symbol.lua", "../../addon", path).CombinedOutput()
			if err != nil {
				t.Fatalf("%v\n%s", err, output)
			}
			var drawings struct{ First, Second, Paired symbolDrawing }
			if err := json.Unmarshal(output, &drawings); err != nil {
				t.Fatal(err)
			}
			for i, drawing := range []symbolDrawing{drawings.First, drawings.Second, drawings.Paired} {
				runs := parseSymbolRuns(t, drawing.Runs)
				module := 0
				if len(runs) > 0 {
					module = runs[0].h
				}
				// Symbols stack vertically in one card, so a paired receipt is
				// taller than it is wide and a single symbol is square. Both
				// stay inside the card budget and keep a 4-module quiet ring.
				if module < 2 || drawing.Width < 29*module || drawing.Width > 1480 || drawing.Height < 29*module || drawing.Height > 1480 || len(drawing.Runs) > 16384 || (i < 2 && drawing.Width != drawing.Height) || (i == 2 && drawing.Height <= drawing.Width) {
					t.Fatalf("invalid drawing: %dx%d module %d", drawing.Width, drawing.Height, module)
				}
				pixels := image.NewNRGBA(image.Rect(0, 0, drawing.Width, drawing.Height))
				for y := 0; y < drawing.Height; y++ {
					for x := 0; x < drawing.Width; x++ {
						pixels.SetNRGBA(x, y, color.NRGBA{255, 255, 255, 255})
					}
				}
				for _, run := range runs {
					if symbolRunInvalid(run, module, drawing.Width, drawing.Height) {
						t.Fatalf("quiet zone or run invalid: %+v", run)
					}
					for yy := run.y; yy < run.y+run.h; yy++ {
						for xx := run.x; xx < run.x+run.w; xx++ {
							pixels.SetNRGBA(xx, yy, color.NRGBA{0, 0, 0, 255})
						}
					}
				}
				texts, err := desktop.DecodeSymbols(pixels)
				// DecodeSymbols preserves transmitted bytes by reading byte mode
				// with ISO-8859-1, so the payload is recovered with
				// BytesFromSymbolText, one rune per byte. Comparing the reader's
				// text form directly would pass only for pure ASCII.
				expected := []string{payload}
				if i == 1 {
					expected = []string{"small"}
				} else if i == 2 {
					expected = append(expected, "next-ready")
				}
				recovered := make([]string, 0, len(texts))
				for _, text := range texts {
					recovered = append(recovered, string(desktop.BytesFromSymbolText(text)))
				}
				slices.Sort(expected)
				slices.Sort(recovered)
				if err != nil || !slices.Equal(recovered, expected) {
					t.Fatalf("decode mismatch: %v, got %q, want %q", err, recovered, expected)
				}
			}
		})
	}
}
