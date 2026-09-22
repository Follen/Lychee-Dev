package video_test

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

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
	payload := append([]byte(listType), body...)
	return chunk("LIST", payload)
}

// buildAVI assembles a RIFF/AVI container with one video stream (no strh, like
// the minimal sample) and "00db" video chunks.
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

func fixturePath(t *testing.T, parts ...string) string {
	t.Helper()
	return filepath.Join(append([]string{"..", "..", "..", "tests", "fixtures"}, parts...)...)
}

// legacyOracle is the recorded legacy "video demux" result for the minimal
// sample. It is used as a cross-check oracle: the new demuxer must agree on the
// legacy metadata fields even though the legacy tool wrote no frame files.
type legacyOracle struct {
	Stdout string `json:"stdout"`
}

type legacyResponse struct {
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

func loadOracle(t *testing.T) legacyResponse {
	t.Helper()
	raw, err := os.ReadFile(fixturePath(t, "oracles", "demux-minimal-vp9.json"))
	if err != nil {
		t.Fatal(err)
	}
	var record legacyOracle
	if err := json.Unmarshal(raw, &record); err != nil {
		t.Fatal(err)
	}
	var response legacyResponse
	if err := json.Unmarshal([]byte(record.Stdout), &response); err != nil {
		t.Fatal(err)
	}
	return response
}

func loadSample(t *testing.T) []byte {
	t.Helper()
	raw, err := os.ReadFile(fixturePath(t, "inputs", "minimal-vp9.avi"))
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestParseAVIMinimalSampleMatchesLegacyOracle(t *testing.T) {
	sample := loadSample(t)
	want := loadOracle(t).Data
	got, err := video.ParseAVI(sample, video.Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if got.Width != want.Width || got.Height != want.Height || got.FrameRate != want.FrameRate || got.FrameCount != want.FrameCount {
		t.Fatalf("metadata = %dx%d @%v x%d, want %dx%d @%v x%d",
			got.Width, got.Height, got.FrameRate, got.FrameCount,
			want.Width, want.Height, want.FrameRate, want.FrameCount)
	}
	if len(got.Frames) != len(want.Frames) {
		t.Fatalf("frames = %d, want %d", len(got.Frames), len(want.Frames))
	}
	for i, frame := range got.Frames {
		legacy := want.Frames[i]
		if frame.Type != legacy.Type || frame.Timestamp != legacy.Timestamp || frame.Duration != legacy.Duration || frame.Size != legacy.Size {
			t.Fatalf("frame %d = %+v, want %+v", i, frame, legacy)
		}
		if len(frame.Payload) != frame.Size {
			t.Fatalf("frame %d payload = %d bytes, want %d", i, len(frame.Payload), frame.Size)
		}
	}
	if !got.Complete || got.Truncated {
		t.Fatalf("completeness = %+v", got)
	}
}

func TestParseAVISyntheticFrameTimeline(t *testing.T) {
	frames := [][]byte{bytesOf(10, 7), bytesOf(3, 1), bytesOf(5, 2)}
	container, err := video.ParseAVI(buildAVI(20000, 640, 480, frames), video.Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if container.FrameRate != 50 || container.Width != 640 || container.Height != 480 {
		t.Fatalf("metadata = %+v", container)
	}
	wantTimes := []float64{0, 20000, 40000}
	for i, frame := range container.Frames {
		if frame.Type != "key" || frame.Timestamp != wantTimes[i] || frame.Duration != 20000 || frame.Size != len(frames[i]) {
			t.Fatalf("frame %d = %+v", i, frame)
		}
		if !reflect.DeepEqual(frame.Payload, frames[i]) {
			t.Fatalf("frame %d payload mismatch", i)
		}
	}
}

func TestParseAVIRejectsTruncatedContainer(t *testing.T) {
	sample := loadSample(t)
	for _, cut := range []int{5, 11, 300, 352} {
		if _, err := video.ParseAVI(sample[:cut], video.Limits{}); !errors.Is(err, video.ErrContainerTruncated) {
			t.Fatalf("cut %d: %v", cut, err)
		}
	}
}

func TestParseAVIRejectsBadChunkSizes(t *testing.T) {
	// A movi chunk whose declared size leaves the container. The RIFF size
	// itself is consistent so the chunk-level check is what must reject it.
	oversizedBody := []byte("AVI LIST")
	oversizedBody = binary.LittleEndian.AppendUint32(oversizedBody, 0xFFFFFFF0)
	oversizedBody = append(oversizedBody, []byte("movi00db")...)
	oversizedBody = binary.LittleEndian.AppendUint32(oversizedBody, 0xFFFFFFF0)
	oversized := []byte("RIFF")
	oversized = binary.LittleEndian.AppendUint32(oversized, uint32(len(oversizedBody)))
	oversized = append(oversized, oversizedBody...)
	if _, err := video.ParseAVI(oversized, video.Limits{}); !errors.Is(err, video.ErrContainerTruncated) {
		t.Fatalf("oversized chunk: %v", err)
	}
	// A LIST smaller than its own type field is structurally invalid.
	badList := []byte("RIFF")
	badListBody := []byte("AVI LIST")
	badListBody = binary.LittleEndian.AppendUint32(badListBody, 2)
	badListBody = append(badListBody, []byte("mo")...)
	badList = binary.LittleEndian.AppendUint32(badList, uint32(len(badListBody)))
	badList = append(badList, badListBody...)
	if _, err := video.ParseAVI(badList, video.Limits{}); !errors.Is(err, video.ErrContainerFormat) {
		t.Fatalf("short list: %v", err)
	}
	// An avih chunk without its usPerFrame field is invalid.
	shortAvihBody := []byte("AVI avih")
	shortAvihBody = binary.LittleEndian.AppendUint32(shortAvihBody, 2)
	shortAvihBody = append(shortAvihBody, 0, 0)
	shortAvih := []byte("RIFF")
	shortAvih = binary.LittleEndian.AppendUint32(shortAvih, uint32(len(shortAvihBody)))
	shortAvih = append(shortAvih, shortAvihBody...)
	if _, err := video.ParseAVI(shortAvih, video.Limits{}); !errors.Is(err, video.ErrContainerFormat) {
		t.Fatalf("short avih: %v", err)
	}
}

func TestParseAVIRejectsNonAVIInput(t *testing.T) {
	if _, err := video.ParseAVI([]byte("not an avi file"), video.Limits{}); !errors.Is(err, video.ErrContainerFormat) {
		t.Fatalf("garbage: %v", err)
	}
	riffWave := append([]byte("RIFF"), 4, 0, 0, 0)
	riffWave = append(riffWave, []byte("WAVE")...)
	if _, err := video.ParseAVI(riffWave, video.Limits{}); !errors.Is(err, video.ErrContainerFormat) {
		t.Fatalf("wave: %v", err)
	}
}

func TestParseAVIWithoutMovi(t *testing.T) {
	avih := make([]byte, 56)
	hdrl := list("hdrl", chunk("avih", avih))
	body := append([]byte("AVI "), hdrl...)
	out := append([]byte("RIFF"), 0, 0, 0, 0)
	binary.LittleEndian.PutUint32(out[4:8], uint32(len(body)))
	out = append(out, body...)
	if _, err := video.ParseAVI(out, video.Limits{}); !errors.Is(err, video.ErrMoviMissing) {
		t.Fatalf("missing movi: %v", err)
	}
}

func TestParseAVIZeroFrameContainer(t *testing.T) {
	container, err := video.ParseAVI(buildAVI(33333, 16, 16, nil), video.Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if container.FrameCount != 0 || len(container.Frames) != 0 || !container.Complete {
		t.Fatalf("zero-frame container = %+v", container)
	}
}

func TestParseAVIFrameLimitsAreVisible(t *testing.T) {
	frames := [][]byte{bytesOf(10, 1), bytesOf(10, 2), bytesOf(10, 3)}
	sample := buildAVI(33333, 16, 16, frames)
	if _, err := video.ParseAVI(sample, video.Limits{MaxFrames: 2, MaxFrameBytes: 1 << 20, MaxTotalBytes: 1 << 20}); !errors.Is(err, video.ErrFrameLimit) {
		t.Fatalf("strict frame limit: %v", err)
	}
	container, err := video.ParseAVI(sample, video.Limits{MaxFrames: 2, MaxFrameBytes: 1 << 20, MaxTotalBytes: 1 << 20, AllowPartial: true})
	if err != nil {
		t.Fatal(err)
	}
	if len(container.Frames) != 2 || !container.Truncated || container.Complete || container.TruncationReason != "frame_limit" {
		t.Fatalf("partial result = %+v", container)
	}
	if container.EnumeratedFrames != 3 || container.EnumeratedBytes != 30 {
		t.Fatalf("enumeration = %d/%d, want 3/30", container.EnumeratedFrames, container.EnumeratedBytes)
	}
	container, err = video.ParseAVI(sample, video.Limits{MaxFrames: 10, MaxFrameBytes: 1 << 20, MaxTotalBytes: 25, AllowPartial: true})
	if err != nil {
		t.Fatal(err)
	}
	if len(container.Frames) != 2 || container.TruncationReason != "byte_limit" {
		t.Fatalf("byte limit = %+v", container)
	}
	// A single oversized frame always fails, even with partial output allowed.
	if _, err := video.ParseAVI(sample, video.Limits{MaxFrameBytes: 5, AllowPartial: true}); !errors.Is(err, video.ErrFrameLimit) {
		t.Fatalf("oversized frame: %v", err)
	}
}

func bytesOf(size int, fill byte) []byte {
	out := make([]byte, size)
	for i := range out {
		out[i] = fill
	}
	return out
}

func TestSuperframeSizes(t *testing.T) {
	frameA := bytesOf(10, 0xAA)
	frameB := bytesOf(20, 0xBB)
	superframe := append(append(append([]byte{}, frameA...), frameB...), 0xC1, 10, 20, 0xC1)
	if got := video.SuperframeSizes(superframe); !reflect.DeepEqual(got, []int{10, 20}) {
		t.Fatalf("superframe sizes = %v", got)
	}
	duplicated := append(append(append([]byte{}, frameA...), frameB...), 0xC1, 10, 20, 10, 20, 0xC1)
	if got := video.SuperframeSizes(duplicated); !reflect.DeepEqual(got, []int{10, 20}) {
		t.Fatalf("duplicated index sizes = %v", got)
	}
	// Declared sizes that do not cover the payload are not trusted.
	badSum := append(bytesOf(30, 0xAA), 0xC1, 9, 20, 0xC1)
	if got := video.SuperframeSizes(badSum); got != nil {
		t.Fatalf("invalid index accepted: %v", got)
	}
	if got := video.SuperframeSizes(bytesOf(64, 0)); got != nil {
		t.Fatalf("plain payload accepted: %v", got)
	}
	single := append(bytesOf(10, 0xAA), 0xC0, 10, 0xC0)
	if got := video.SuperframeSizes(single); got != nil {
		t.Fatalf("single-frame index accepted: %v", got)
	}
}
