package command

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

// These checks consume retained bytes and the recorded oracle as data. They do
// not invoke the retired executable; the legacy result is only a golden value.
func TestDemuxGoldenThroughCurrentCLI(t *testing.T) {
	root := filepath.Clean(filepath.Join("..", ".."))
	fixture := filepath.Join(root, "tests", "fixtures", "inputs", "minimal-vp9.avi")
	var recorded struct {
		ExitCode int    `json:"exitCode"`
		Stdout   string `json:"stdout"`
		Stderr   string `json:"stderr"`
	}
	oracle, err := os.ReadFile(filepath.Join(root, "tests", "fixtures", "oracles", "demux-minimal-vp9.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(oracle, &recorded); err != nil {
		t.Fatal(err)
	}
	var want struct {
		OK   bool `json:"ok"`
		Data struct {
			FrameCount int     `json:"frameCount"`
			FrameRate  float64 `json:"frameRate"`
			Width      int     `json:"width"`
			Height     int     `json:"height"`
			Frames     []struct {
				Type      string  `json:"type"`
				Timestamp float64 `json:"timestamp"`
				Duration  float64 `json:"duration"`
				Size      int     `json:"size"`
			} `json:"frames"`
		} `json:"data"`
		Warnings []string `json:"warnings"`
	}
	if err := json.Unmarshal([]byte(recorded.Stdout), &want); err != nil {
		t.Fatal(err)
	}
	if recorded.ExitCode != 0 || recorded.Stderr != "" || !want.OK {
		t.Fatalf("invalid retained oracle record: %+v", recorded)
	}

	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	home := t.TempDir()
	if _, err := vault.Initialize(context.Background(), home); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr bytes.Buffer
	code := Execute(context.Background(), []string{"asset", "demux", "--path", fixture, "--output", output, "--home", home, "--format=json"}, &stdout, &stderr)
	if code != 0 || stderr.Len() != 0 {
		t.Fatalf("current CLI code=%d stderr=%q stdout=%s", code, stderr.String(), stdout.String())
	}
	var envelope struct {
		Schema   string          `json:"schema"`
		OK       bool            `json:"ok"`
		Result   json.RawMessage `json:"result"`
		Warnings []string        `json:"warnings"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &envelope); err != nil {
		t.Fatal(err)
	}
	if envelope.Schema != "lycheedev.result.v1" || !envelope.OK {
		t.Fatalf("CLI envelope: %s", stdout.String())
	}
	var result map[string]any
	if err := json.Unmarshal(envelope.Result, &result); err != nil {
		t.Fatal(err)
	}
	for key, expected := range map[string]any{"schema": "lycheedev.asset-demux.v1", "frameCount": float64(want.Data.FrameCount), "frameRate": want.Data.FrameRate, "width": float64(want.Data.Width), "height": float64(want.Data.Height), "complete": true, "truncated": false} {
		if result[key] != expected {
			t.Errorf("CLI result[%q]=%v, want %v", key, result[key], expected)
		}
	}
	frames, ok := result["frames"].([]any)
	if !ok || len(frames) != len(want.Data.Frames) {
		t.Fatalf("CLI frames=%v, want %d", result["frames"], len(want.Data.Frames))
	}
	for i, item := range frames {
		frame := item.(map[string]any)
		w := want.Data.Frames[i]
		if frame["type"] != w.Type || frame["timestamp"] != w.Timestamp || frame["duration"] != w.Duration || frame["size"] != float64(w.Size) {
			t.Errorf("CLI frame %d=%v, oracle=%+v", i, frame, w)
		}
	}
	if len(want.Warnings) != 0 {
		t.Fatalf("unexpected oracle warnings: %v", want.Warnings)
	}
	if len(envelope.Warnings) != 0 {
		t.Fatalf("unexpected CLI warnings: %v", envelope.Warnings)
	}
}
