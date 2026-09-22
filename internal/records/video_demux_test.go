package records_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/video"
)

func chunk(id string, payload []byte) []byte {
	out := append([]byte(id), 0, 0, 0, 0)
	binary.LittleEndian.PutUint32(out[4:8], uint32(len(payload)))
	out = append(out, payload...)
	if len(payload)%2 == 1 {
		out = append(out, 0)
	}
	return out
}

func list(listType string, body []byte) []byte {
	return chunk("LIST", append([]byte(listType), body...))
}

func buildAVI(usPerFrame, width, height uint32, frames [][]byte) []byte {
	avih := make([]byte, 56)
	binary.LittleEndian.PutUint32(avih[0:4], usPerFrame)
	strf := make([]byte, 40)
	binary.LittleEndian.PutUint32(strf[4:8], width)
	binary.LittleEndian.PutUint32(strf[8:12], height)
	hdrl := list("hdrl", append(chunk("avih", avih), list("strl", chunk("strf", strf))...))
	var moviBody []byte
	for _, frame := range frames {
		moviBody = append(moviBody, chunk("00db", frame)...)
	}
	body := append([]byte("AVI "), hdrl...)
	body = append(body, list("movi", moviBody)...)
	out := []byte("RIFF")
	out = binary.LittleEndian.AppendUint32(out, uint32(len(body)))
	return append(out, body...)
}

func samplePath(t *testing.T) string {
	t.Helper()
	return filepath.Join("..", "..", "tests", "fixtures", "inputs", "minimal-vp9.avi")
}

func sampleAVI(t *testing.T) []byte {
	t.Helper()
	raw, err := os.ReadFile(samplePath(t))
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// legacyDemuxOracle is the recorded legacy "video demux" result used as a
// cross-check oracle (the legacy tool wrote no frame files).
type legacyDemuxOracle struct {
	Stdout string `json:"stdout"`
}

type legacyDemuxResponse struct {
	Data struct {
		FrameCount int     `json:"frameCount"`
		FrameRate  float64 `json:"frameRate"`
		Height     int     `json:"height"`
		Width      int     `json:"width"`
		Frames     []struct {
			Type      string  `json:"type"`
			Timestamp float64 `json:"timestamp"`
			Duration  float64 `json:"duration"`
			Size      int     `json:"size"`
		} `json:"frames"`
	} `json:"data"`
}

func legacyDemuxOracleData(t *testing.T) legacyDemuxResponse {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "..", "tests", "fixtures", "oracles", "demux-minimal-vp9.json"))
	if err != nil {
		t.Fatal(err)
	}
	var record legacyDemuxOracle
	if err := json.Unmarshal(raw, &record); err != nil {
		t.Fatal(err)
	}
	var response legacyDemuxResponse
	if err := json.Unmarshal([]byte(record.Stdout), &response); err != nil {
		t.Fatal(err)
	}
	return response
}

func checkAgainstOracle(t *testing.T, manifest records.AssetDemuxManifest) {
	t.Helper()
	want := legacyDemuxOracleData(t).Data
	if manifest.Width != want.Width || manifest.Height != want.Height || manifest.FrameRate != want.FrameRate || manifest.FrameCount != want.FrameCount {
		t.Fatalf("metadata = %dx%d @%v x%d, want %dx%d @%v x%d",
			manifest.Width, manifest.Height, manifest.FrameRate, manifest.FrameCount,
			want.Width, want.Height, want.FrameRate, want.FrameCount)
	}
	if len(manifest.Frames) != len(want.Frames) {
		t.Fatalf("frames = %d, want %d", len(manifest.Frames), len(want.Frames))
	}
	for i, frame := range manifest.Frames {
		legacy := want.Frames[i]
		if frame.Type != legacy.Type || frame.Timestamp != legacy.Timestamp || frame.Duration != legacy.Duration || frame.Size != legacy.Size {
			t.Fatalf("frame %d = %+v, want %+v", i, frame, legacy)
		}
	}
}

func publishedFiles(t *testing.T, output string) map[string][]byte {
	t.Helper()
	entries, err := os.ReadDir(output)
	if err != nil {
		t.Fatal(err)
	}
	files := map[string][]byte{}
	for _, entry := range entries {
		raw, err := os.ReadFile(filepath.Join(output, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		files[entry.Name()] = raw
	}
	return files
}

func TestDemuxAssetLocalSampleMatchesLegacyOracle(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	sample := sampleAVI(t)
	demux, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{
		Path: samplePath(t), Output: output,
	})
	if err != nil {
		t.Fatal(err)
	}
	checkAgainstOracle(t, demux.Manifest)
	if demux.Manifest.Schema != records.AssetDemuxSchema || !demux.Manifest.Complete || demux.Manifest.Source.Kind != "file" {
		t.Fatalf("manifest = %+v", demux.Manifest)
	}
	if demux.Manifest.Source.SHA256 != sha256hex(sample) || demux.Manifest.Source.Bytes != int64(len(sample)) {
		t.Fatalf("source identity = %+v", demux.Manifest.Source)
	}
	files := publishedFiles(t, output)
	if len(files) != 3 {
		t.Fatalf("published = %v", keysOf(files))
	}
	for i, frame := range demux.Manifest.Frames {
		raw, ok := files[frame.Output]
		if !ok {
			t.Fatalf("missing frame file %s", frame.Output)
		}
		if len(raw) != frame.Size || sha256hex(raw) != frame.SHA256 {
			t.Fatalf("frame %d file mismatch", i)
		}
		if frame.Capture == "" || frame.Blob.SHA256 != frame.SHA256 {
			t.Fatalf("frame %d evidence = %+v", i, frame)
		}
	}
	manifestBytes, err := json.MarshalIndent(demux.Manifest, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(files["demux-manifest.json"], manifestBytes) {
		t.Fatalf("published manifest diverges:\n%s\n%s", files["demux-manifest.json"], manifestBytes)
	}
	archive := openEvidence(t, workspace)
	if err := archive.VerifyCapture(ctx, demux.Capture.ID); err != nil {
		t.Fatal(err)
	}
	if err := archive.VerifyCapture(ctx, demux.Source.ID); err != nil {
		t.Fatal(err)
	}
	_, capturedSource, err := archive.FetchCapture(ctx, demux.Source.ID, 1<<20)
	if err != nil || !bytes.Equal(capturedSource, sample) {
		t.Fatalf("source capture: %v", err)
	}
	for i, capture := range demux.Frames {
		ref, body, err := archive.FetchCapture(ctx, capture.ID, 1<<20)
		if err != nil {
			t.Fatal(err)
		}
		if ref.Provenance.Kind != "asset-demux-frame" || sha256hex(body) != demux.Manifest.Frames[i].SHA256 {
			t.Fatalf("frame capture %d = %+v", i, ref)
		}
	}
	if len(demux.Published) != 3 {
		t.Fatalf("published list = %v", demux.Published)
	}
}

func TestDemuxAssetCASCInput(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	sample := sampleAVI(t)
	pin := cachedAssetPin(t, workspace, sample)
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	demux, err := records.DemuxAsset(ctx, workspace, pin.ID, records.AssetDemuxRequest{
		File: cdnQuery(11), Output: output,
	})
	if err != nil {
		t.Fatal(err)
	}
	checkAgainstOracle(t, demux.Manifest)
	if demux.Manifest.Source.Kind != "casc" || demux.Manifest.Source.FileDataID != 11 || demux.Manifest.Source.Snapshot != pin.ID ||
		demux.Manifest.Source.Pin == nil || *demux.Manifest.Source.Pin != *pin.Data || demux.Manifest.Input != "casc:fdid:11" {
		t.Fatalf("source identity = %+v", demux.Manifest.Source)
	}
	if demux.Manifest.Source.ContentKey != md5hex(sample) {
		t.Fatalf("content key = %+v", demux.Manifest.Source)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{File: cdnQuery(11), Output: output}); !errors.Is(err, records.ErrSnapshotRequired) {
		t.Fatalf("missing snapshot: %v", err)
	}
}

func TestDemuxAssetOverwriteRefusalAndExplicitReplace(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	request := records.AssetDemuxRequest{Path: samplePath(t), Output: output}
	if _, err := records.DemuxAsset(ctx, workspace, "", request); err != nil {
		t.Fatal(err)
	}
	before := publishedFiles(t, output)
	// Default refusal: no existing file is ever replaced or truncated.
	if _, err := records.DemuxAsset(ctx, workspace, "", request); !errors.Is(err, records.ErrExportConflict) {
		t.Fatalf("overwrite refusal: %v", err)
	}
	after := publishedFiles(t, output)
	for name, raw := range before {
		if !bytes.Equal(after[name], raw) {
			t.Fatalf("file %s changed on refused run", name)
		}
	}
	request.Overwrite = true
	if _, err := records.DemuxAsset(ctx, workspace, "", request); err != nil {
		t.Fatal(err)
	}
	if len(publishedFiles(t, output)) != 3 {
		t.Fatal("explicit overwrite lost files")
	}
}

func TestDemuxAssetCancellationLeavesOutputUntouched(t *testing.T) {
	workspace := workspaceRoot(t)
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{
		Path: samplePath(t), Output: output,
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation: %v", err)
	}
	if files := publishedFiles(t, output); len(files) != 0 {
		t.Fatalf("cancellation published %v", keysOf(files))
	}
}

func TestDemuxAssetFrameLimitsAreVisible(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	sample := buildAVI(33333, 16, 16, [][]byte{bytesOf(10, 1), bytesOf(10, 2), bytesOf(10, 3)})
	path := filepath.Join(t.TempDir(), "synthetic.avi")
	if err := os.WriteFile(path, sample, 0600); err != nil {
		t.Fatal(err)
	}
	strictOutput := filepath.Join(t.TempDir(), "strict")
	partialOutput := filepath.Join(t.TempDir(), "partial")
	for _, dir := range []string{strictOutput, partialOutput} {
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
	}
	request := records.AssetDemuxRequest{Path: path, Output: strictOutput, MaxFrames: 2, MaxFrameBytes: 1 << 20, MaxOutputBytes: 1 << 20}
	if _, err := records.DemuxAsset(ctx, workspace, "", request); !errors.Is(err, records.ErrFrameLimit) {
		t.Fatalf("strict limit: %v", err)
	}
	if files := publishedFiles(t, strictOutput); len(files) != 0 {
		t.Fatalf("strict limit published %v", keysOf(files))
	}
	request.Output = partialOutput
	request.AllowPartial = true
	demux, err := records.DemuxAsset(ctx, workspace, "", request)
	if err != nil {
		t.Fatal(err)
	}
	manifest := demux.Manifest
	if !manifest.Truncated || manifest.Complete || manifest.TruncationReason != "frame_limit" || manifest.FrameCount != 2 || manifest.EnumeratedFrames != 3 {
		t.Fatalf("partial manifest = %+v", manifest)
	}
	files := publishedFiles(t, partialOutput)
	if len(files) != 3 {
		t.Fatalf("partial published %v", keysOf(files))
	}
	if archive := openEvidence(t, workspace); archive.VerifyCapture(ctx, demux.Capture.ID) != nil {
		t.Fatal("manifest capture failed verification")
	}
	// An oversized single frame is a precise error even with partial output.
	oversizedOutput := filepath.Join(t.TempDir(), "oversized")
	if err := os.Mkdir(oversizedOutput, 0700); err != nil {
		t.Fatal(err)
	}
	request.Output = oversizedOutput
	request.MaxFrames = 10
	request.MaxFrameBytes = 5
	if _, err := records.DemuxAsset(ctx, workspace, "", request); !errors.Is(err, records.ErrFrameLimit) {
		t.Fatalf("oversized frame: %v", err)
	}
	if files := publishedFiles(t, oversizedOutput); len(files) != 0 {
		t.Fatalf("oversized frame published %v", keysOf(files))
	}
}

func TestDemuxAssetMalformedContainers(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	// Zero-frame containers are valid and publish an empty manifest.
	zero := filepath.Join(t.TempDir(), "zero.avi")
	if err := os.WriteFile(zero, buildAVI(33333, 8, 8, nil), 0600); err != nil {
		t.Fatal(err)
	}
	demux, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: zero, Output: output})
	if err != nil {
		t.Fatal(err)
	}
	if demux.Manifest.FrameCount != 0 || !demux.Manifest.Complete || len(demux.Published) != 1 {
		t.Fatalf("zero-frame demux = %+v", demux.Published)
	}
	// Truncated containers and bad chunk sizes fail precisely and publish
	// nothing new.
	sample := sampleAVI(t)
	truncatedPath := filepath.Join(t.TempDir(), "truncated.avi")
	if err := os.WriteFile(truncatedPath, sample[:300], 0600); err != nil {
		t.Fatal(err)
	}
	freshOutput := filepath.Join(t.TempDir(), "fresh")
	if err := os.Mkdir(freshOutput, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: truncatedPath, Output: freshOutput}); !errors.Is(err, video.ErrContainerTruncated) {
		t.Fatalf("truncated container: %v", err)
	}
	badSize := filepath.Join(t.TempDir(), "bad-size.avi")
	badBody := []byte("AVI LIST")
	badBody = binary.LittleEndian.AppendUint32(badBody, 0xFFFFFFF0)
	badBody = append(badBody, []byte("movi00db")...)
	badBody = binary.LittleEndian.AppendUint32(badBody, 0xFFFFFFF0)
	badFile := binary.LittleEndian.AppendUint32([]byte("RIFF"), uint32(len(badBody)))
	badFile = append(badFile, badBody...)
	if err := os.WriteFile(badSize, badFile, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: badSize, Output: freshOutput}); !errors.Is(err, video.ErrContainerTruncated) {
		t.Fatalf("bad chunk size: %v", err)
	}
	if files := publishedFiles(t, freshOutput); len(files) != 0 {
		t.Fatalf("failed runs published %v", keysOf(files))
	}
}

func TestDemuxAssetOutputContract(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	sample := sampleAVI(t)
	path := filepath.Join(t.TempDir(), "in.avi")
	if err := os.WriteFile(path, sample, 0600); err != nil {
		t.Fatal(err)
	}
	// The output directory must exist.
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: path, Output: filepath.Join(t.TempDir(), "missing")}); !errors.Is(err, records.ErrExportPath) {
		t.Fatalf("missing output directory: %v", err)
	}
	// Outputs must stay outside the managed workspace.
	inside := filepath.Join(workspace, "frames")
	if err := os.Mkdir(inside, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: path, Output: inside}); !errors.Is(err, records.ErrExportPath) {
		t.Fatalf("workspace output: %v", err)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Path: path}); !errors.Is(err, records.ErrExportPath) {
		t.Fatalf("missing output: %v", err)
	}
	if _, err := records.DemuxAsset(ctx, workspace, "", records.AssetDemuxRequest{Output: inside}); !errors.Is(err, records.ErrFileQuery) {
		t.Fatalf("missing input: %v", err)
	}
}

func sha256hex(raw []byte) string {
	digest := sha256.Sum256(raw)
	return hex.EncodeToString(digest[:])
}

func bytesOf(size int, fill byte) []byte {
	out := make([]byte, size)
	for i := range out {
		out[i] = fill
	}
	return out
}

func keysOf(files map[string][]byte) []string {
	keys := make([]string, 0, len(files))
	for name := range files {
		keys = append(keys, name)
	}
	return keys
}
