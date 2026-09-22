package command

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
	"golang.org/x/image/webp"
)

func syntheticImageBLP2() ([]byte, []byte, []byte) {
	// The expected RGBA pixels are intentionally independent of the BLP byte
	// order below. The second level is a distinct 1x1 mip for CLI selection.
	wantLevel0 := []byte{
		255, 0, 0, 255,
		0, 255, 0, 128,
		0, 0, 255, 0,
		17, 34, 51, 68,
	}
	wantLevel1 := []byte{9, 8, 7, 6}
	raw := make([]byte, 148+len(wantLevel0)+len(wantLevel1))
	copy(raw, "BLP2")
	binary.LittleEndian.PutUint32(raw[4:], 1)
	raw[8], raw[9], raw[10], raw[11] = 3, 8, 0, 0x11
	binary.LittleEndian.PutUint32(raw[12:], 2)
	binary.LittleEndian.PutUint32(raw[16:], 2)
	binary.LittleEndian.PutUint32(raw[20:], 148)
	binary.LittleEndian.PutUint32(raw[84:], uint32(len(wantLevel0)))
	binary.LittleEndian.PutUint32(raw[24:], 148+uint32(len(wantLevel0)))
	binary.LittleEndian.PutUint32(raw[88:], uint32(len(wantLevel1)))
	copy(raw[148:], []byte{
		0, 0, 255, 255,
		0, 255, 0, 128,
		255, 0, 0, 0,
		51, 34, 17, 68,
	})
	copy(raw[148+len(wantLevel0):], []byte{7, 8, 9, 6})
	return raw, wantLevel0, wantLevel1
}

func imageExportFixture(t *testing.T, content []byte) (workspace, project, snapshot string) {
	t.Helper()
	workspace = filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(t.Context(), workspace); err != nil {
		t.Fatal(err)
	}
	pin := testkit.CachedAsset(t, workspace, content)
	project = filepath.Join(t.TempDir(), "project")
	if err := os.Mkdir(project, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := selection.InitializeProject(t.Context(), project, "retail"); err != nil {
		t.Fatal(err)
	}
	if _, err := selection.LockProject(t.Context(), workspace, project, pin.ID); err != nil {
		t.Fatal(err)
	}
	return workspace, project, pin.ID
}

func invokeAssetImage(t *testing.T, args ...string) (Envelope, int) {
	t.Helper()
	node, launcher := os.Getenv("LYCHEEDEV_ASSET_NODE"), os.Getenv("LYCHEEDEV_ASSET_LAUNCHER")
	if (node == "") != (launcher == "") {
		t.Fatal("both installed asset test launcher variables are required")
	}
	if launcher == "" {
		return invoke(t, args...)
	}
	t.Logf("using installed asset launcher: %s", launcher)
	return invokeExecutable(t, node, append([]string{launcher}, args...)...)
}

func decodeAssetImage(t *testing.T, encoding string, encoded []byte) image.Image {
	t.Helper()
	var (
		decoded image.Image
		err     error
	)
	if encoding == "png" {
		decoded, err = png.Decode(bytes.NewReader(encoded))
	} else {
		decoded, err = webp.Decode(bytes.NewReader(encoded))
	}
	if err != nil {
		t.Fatal(err)
	}
	return decoded
}

func assertDecodedPixels(t *testing.T, decoded image.Image, want []byte) {
	t.Helper()
	width := imageWidth(want)
	if decoded.Bounds() != image.Rect(0, 0, width, len(want)/(width*4)) {
		t.Fatalf("bounds = %v, want %dx%d", decoded.Bounds(), width, len(want)/(width*4))
	}
	for i := 0; i < len(want)/4; i++ {
		x, y := i%width, i/width
		got := color.NRGBAModel.Convert(decoded.At(x, y)).(color.NRGBA)
		actual := []byte{got.R, got.G, got.B, got.A}
		if !bytes.Equal(actual, want[i*4:i*4+4]) {
			t.Fatalf("pixel (%d,%d) = %v, want %v", x, y, actual, want[i*4:i*4+4])
		}
	}
}

func imageWidth(pixels []byte) int {
	if len(pixels) == 4 {
		return 1
	}
	return 2
}

func TestAssetImageExportProjectOffline(t *testing.T) {
	raw, wantLevel0, wantLevel1 := syntheticImageBLP2()
	workspace, project, snapshot := imageExportFixture(t, raw)
	alphaLevel0 := []byte{
		255, 255, 255, 255,
		128, 128, 128, 255,
		0, 0, 0, 255,
		68, 68, 68, 255,
	}

	for _, tc := range []struct {
		encoding string
		mipmap   int
		channels string
		want     []byte
	}{
		{encoding: "png", want: wantLevel0},
		{encoding: "webp", want: wantLevel0},
		{encoding: "webp", channels: "a", want: alphaLevel0},
		{encoding: "png", mipmap: 1, want: wantLevel1},
	} {
		t.Run(tc.encoding+"/mip"+strconv.Itoa(tc.mipmap)+"/"+tc.channels, func(t *testing.T) {
			output := filepath.Join(t.TempDir(), tc.encoding+"-asset")
			args := []string{
				"asset", "export", "--project", project, "--cdn", "--offline", "--file-id", "11",
				"--output", output, "--home", workspace, "--encoding", tc.encoding,
				"--mipmap", strconv.Itoa(tc.mipmap), "--max-pixels", strconv.Itoa(len(tc.want) / 4), "--format=json",
			}
			if tc.channels != "" {
				args = append(args, "--channels", tc.channels)
			}
			result, code := invokeAssetImage(t, args...)
			if code != 0 || !result.OK || result.Context["snapshot"] != snapshot || len(result.Captures) != 3 {
				t.Fatalf("%+v (%d)", result, code)
			}
			manifest := result.Result.(map[string]any)
			if manifest["schema"] != "lycheedev.asset-export.v1" || manifest["encoding"] != tc.encoding || manifest["snapshot"] != snapshot {
				t.Fatal(manifest)
			}
			imageInfo := manifest["image"].(map[string]any)
			texture := imageInfo["texture"].(map[string]any)
			if imageInfo["channels"] != func() string {
				if tc.channels == "" {
					return "rgba"
				}
				return tc.channels
			}() || texture["format"] != "BLP2" || texture["storage"] != "bgra8" || texture["width"] != float64(imageWidth(tc.want)) || texture["height"] != float64(len(tc.want)/(imageWidth(tc.want)*4)) || texture["mipmap"] != float64(tc.mipmap) || texture["levels"] != float64(2) || texture["alphaBits"] != float64(8) {
				t.Fatalf("wrong image metadata: %+v", imageInfo)
			}
			pixelDigest := sha256.Sum256(tc.want)
			if imageInfo["pixelsSHA256"] != hex.EncodeToString(pixelDigest[:]) {
				t.Fatalf("wrong semantic pixel digest: %+v", imageInfo)
			}
			encoded, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			assertDecodedPixels(t, decodeAssetImage(t, tc.encoding, encoded), tc.want)
			content := manifest["content"].(map[string]any)
			artifactDigest := sha256.Sum256(encoded)
			if content["sha256"] != hex.EncodeToString(artifactDigest[:]) || content["bytes"] != float64(len(encoded)) {
				t.Fatalf("wrong artifact content: %+v", content)
			}
			source := manifest["source"].(map[string]any)
			sourceContent := source["content"].(map[string]any)
			sourceDigest := sha256.Sum256(raw)
			if source["source"] != "cdn" || source["entry"].(map[string]any)["fileDataID"] != float64(11) || sourceContent["sha256"] != hex.EncodeToString(sourceDigest[:]) || sourceContent["bytes"] != float64(len(raw)) {
				t.Fatalf("wrong source content: %+v", source)
			}
			sourceCapture := manifest["sourceCapture"].(map[string]any)
			artifactCapture := manifest["artifactCapture"].(map[string]any)
			captures := result.Captures
			if captures[0].(map[string]any)["id"] != sourceCapture["id"] || captures[2].(map[string]any)["id"] != artifactCapture["id"] {
				t.Fatalf("capture ordering/linkage: %+v", captures)
			}
			for _, capture := range captures {
				ref := capture.(map[string]any)
				verified, verifyCode := invokeAssetImage(t, "evidence", "verify", ref["id"].(string), "--home", workspace, "--format=json")
				if verifyCode != 0 || !verified.OK {
					t.Fatalf("capture %s: %+v (%d)", ref["id"], verified, verifyCode)
				}
			}
		})
	}
}

func TestAssetImageExportFailuresPreserveOutput(t *testing.T) {
	raw, _, _ := syntheticImageBLP2()
	workspace, project, _ := imageExportFixture(t, raw)
	output := filepath.Join(t.TempDir(), "keep-image")
	wantExisting := []byte("existing image output")
	if err := os.WriteFile(output, wantExisting, 0600); err != nil {
		t.Fatal(err)
	}
	base := []string{
		"asset", "export", "--project", project, "--cdn", "--offline", "--file-id", "11", "--output", output,
		"--home", workspace, "--encoding", "png", "--overwrite", "--format=json",
	}
	for _, tc := range []struct {
		name  string
		extra []string
		code  int
		fault string
	}{
		{name: "absent-mipmap", extra: []string{"--mipmap", "2"}, code: 3, fault: "texture.mipmap_unavailable"},
		{name: "pixel-budget", extra: []string{"--max-pixels", "1"}, code: 3, fault: "texture.pixel_limit"},
		{name: "export-options", extra: []string{"--channels", "rr"}, code: 2, fault: "records.export_options"},
		{name: "input-byte-budget", extra: []string{"--max-bytes", "1"}, code: 3, fault: "records.metadata_limit"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			result, code := invokeAssetImage(t, append(append([]string{}, base...), tc.extra...)...)
			if code != tc.code || result.OK || result.Error == nil || result.Error.Code != tc.fault || result.Result != nil || len(result.Captures) != 0 {
				t.Fatalf("%+v (%d)", result, code)
			}
			actual, err := os.ReadFile(output)
			if err != nil || !bytes.Equal(actual, wantExisting) {
				t.Fatalf("failed export changed output: %q (%v)", actual, err)
			}
		})
	}

	badRaw := []byte("BLP2")
	badWorkspace, badProject, _ := imageExportFixture(t, badRaw)
	badOutput := filepath.Join(t.TempDir(), "keep-invalid")
	if err := os.WriteFile(badOutput, wantExisting, 0600); err != nil {
		t.Fatal(err)
	}
	badArgs := []string{
		"asset", "export", "--project", badProject, "--cdn", "--offline", "--file-id", "11", "--output", badOutput,
		"--home", badWorkspace, "--encoding", "png", "--overwrite", "--format=json",
	}
	result, code := invokeAssetImage(t, badArgs...)
	if code != 4 || result.OK || result.Error == nil || result.Error.Code != "texture.invalid_format" || result.Result != nil || len(result.Captures) != 0 {
		t.Fatalf("%+v (%d)", result, code)
	}
	actual, err := os.ReadFile(badOutput)
	if err != nil || !bytes.Equal(actual, wantExisting) {
		t.Fatalf("invalid image changed output: %q (%v)", actual, err)
	}
}
