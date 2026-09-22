package qrfixture

import (
	"bytes"
	"image"
	"image/color"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
)

// The encoder is the shipped addon/Bridge/MatrixSymbol.lua (the retired 1.x
// Libs/AutomationQR.lua remains accepted); the decoder contract is byte
// preservation, not JSON reserialization. The fixture deliberately includes
// ASCII and UTF-8.: the decoder contract is byte preservation, not
// JSON reserialization. The fixture deliberately includes ASCII and UTF-8.
const qrFixture = `{"v":1,"message":"ASCII + 世界 + café ☕"}`

func TestLuaAutomationQRToGoDecoder(t *testing.T) {
	lua := findLua51(t)
	_, thisFile, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	fixtureDir := filepath.Dir(thisFile)
	root := filepath.Clean(filepath.Join(fixtureDir, "..", ".."))
	harness := filepath.Join(fixtureDir, "harness.lua")
	encoder := filepath.Join(root, "addon", "Bridge", "MatrixSymbol.lua")

	modules := runLuaEncoder(t, lua, harness, encoder, []byte(qrFixture))
	decoded, err := desktop.DecodeSymbols(modulesImage(modules, 4, 8))
	if err != nil {
		t.Fatalf("desktop.DecodeSymbols: %v", err)
	}
	if len(decoded) != 1 {
		t.Fatalf("desktop.DecodeSymbols returned %d symbols, want 1: %#v", len(decoded), decoded)
	}
	if !bytes.Equal([]byte(decoded[0]), []byte(qrFixture)) {
		t.Fatalf("decoded bytes = % x, want % x", []byte(decoded[0]), []byte(qrFixture))
	}
}

func findLua51(t *testing.T) string {
	t.Helper()
	for _, name := range []string{"lua5.1", "lua51", "lua"} {
		path, err := exec.LookPath(name)
		if err != nil {
			continue
		}
		cmd := exec.Command(path, "-v")
		output, err := cmd.CombinedOutput()
		if err == nil && bytes.Contains(output, []byte("Lua 5.1")) {
			return path
		}
	}
	t.Skip("Lua 5.1 is unavailable; skipping offline Lua encoder compatibility test")
	return ""
}

func runLuaEncoder(t *testing.T, lua, harness, encoder string, payload []byte) [][]bool {
	t.Helper()
	cmd := exec.Command(lua, harness, encoder)
	cmd.Stdin = bytes.NewReader(payload)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		t.Fatalf("Lua QR harness failed: %v\n%s", err, stderr.String())
	}
	if stderr.Len() != 0 {
		t.Fatalf("Lua QR harness wrote stderr: %s", stderr.String())
	}
	return parseMatrix(t, stdout.String())
}

func parseMatrix(t *testing.T, output string) [][]bool {
	t.Helper()
	lines := strings.Split(strings.TrimSpace(output), "\n")
	if len(lines) < 3 {
		t.Fatalf("Lua QR harness output is incomplete: %q", output)
	}
	header := strings.Fields(strings.TrimSuffix(lines[0], "\r"))
	if len(header) != 3 || header[0] != "LYCHEE_QR_MATRIX" || header[1] != "1" {
		t.Fatalf("unexpected Lua QR harness header: %q", lines[0])
	}
	width, err := strconv.Atoi(header[2])
	if err != nil || width < 21 {
		t.Fatalf("unexpected matrix width %q", header[2])
	}
	if len(lines) != width+2 || strings.TrimSuffix(lines[len(lines)-1], "\r") != "END" {
		t.Fatalf("unexpected matrix line count: got %d, width %d", len(lines), width)
	}

	modules := make([][]bool, width)
	for y := 0; y < width; y++ {
		row := strings.TrimSuffix(lines[y+1], "\r")
		if len(row) != width {
			t.Fatalf("matrix row %d has %d bytes, want %d", y, len(row), width)
		}
		modules[y] = make([]bool, width)
		for x := 0; x < width; x++ {
			switch row[x] {
			case '0':
			case '1':
				modules[y][x] = true
			default:
				t.Fatalf("matrix row %d contains byte %#x at column %d", y, row[x], x)
			}
		}
	}
	return modules
}

func modulesImage(modules [][]bool, quietZone, scale int) image.Image {
	width := len(modules)
	imageWidth := (width + quietZone*2) * scale
	result := image.NewGray(image.Rect(0, 0, imageWidth, imageWidth))
	for index := range result.Pix {
		result.Pix[index] = color.Gray{Y: 255}.Y
	}
	for y, row := range modules {
		for x, black := range row {
			if !black {
				continue
			}
			left := (x + quietZone) * scale
			top := (y + quietZone) * scale
			for py := top; py < top+scale; py++ {
				for px := left; px < left+scale; px++ {
					result.SetGray(px, py, color.Gray{Y: 0})
				}
			}
		}
	}
	return result
}
