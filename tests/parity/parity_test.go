package parity

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/command"
	semanticparity "github.com/follenfang/lycheedev/internal/parity"
	"github.com/follenfang/lycheedev/internal/records/video"
	"github.com/follenfang/lycheedev/internal/vault"
)

// These checks consume retained bytes and the recorded oracle as data. They do
// not invoke the retired executable; the legacy result is only a golden value.
func TestDemuxGoldenThroughParserAndCurrentCLI(t *testing.T) {
	root := filepath.Clean(filepath.Join("..", ".."))
	fixture := filepath.Join(root, "tests", "fixtures", "inputs", "minimal-vp9.avi")
	raw, err := os.ReadFile(fixture)
	if err != nil {
		t.Fatal(err)
	}
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

	parsed, err := video.ParseAVI(raw, video.Limits{})
	if err != nil {
		t.Fatal(err)
	}
	if parsed.Width != want.Data.Width || parsed.Height != want.Data.Height || parsed.FrameRate != want.Data.FrameRate || parsed.FrameCount != want.Data.FrameCount || !parsed.Complete || parsed.Truncated {
		t.Fatalf("parser metadata/completeness = %+v, want oracle %+v", parsed, want.Data)
	}
	if len(parsed.Frames) != len(want.Data.Frames) {
		t.Fatalf("parser frames=%d, oracle frames=%d", len(parsed.Frames), len(want.Data.Frames))
	}
	for i, got := range parsed.Frames {
		w := want.Data.Frames[i]
		if got.Type != w.Type || got.Timestamp != w.Timestamp || got.Duration != w.Duration || got.Size != w.Size || len(got.Payload) != w.Size {
			t.Errorf("parser frame %d = %+v payload=%d, oracle=%+v", i, got, len(got.Payload), w)
		}
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
	code := command.Execute(context.Background(), []string{"asset", "demux", "--path", fixture, "--output", output, "--home", home, "--format=json"}, &stdout, &stderr)
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

func TestCoverageCatalogIsCompleteAndGrounded(t *testing.T) {
	root := filepath.Clean(filepath.Join("..", ".."))
	catalog, err := semanticparity.Load("coverage.json")
	if err != nil {
		t.Fatal(err)
	}
	commands, err := semanticparity.DiscoverCommands(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	report, err := semanticparity.Validate(catalog, root, commands)
	if err != nil {
		t.Fatal(err)
	}
	if report.Summary.Total != len(catalog.Cases) || report.Summary.Total < 50 {
		t.Fatalf("unexpected parity summary: %+v", report.Summary)
	}
	t.Logf("offline parity inventory: %+v", report.Summary)
	wantNormalizers := []string{"cache-state-v1", "profile-state-v1", "remote-products-v1"}
	got := append([]string(nil), catalog.Normalizers...)
	sort.Strings(got)
	if strings.Join(got, ",") != strings.Join(wantNormalizers, ",") {
		t.Fatalf("normalizer set = %v, want %v", got, wantNormalizers)
	}
}

func TestCoverageCatalogFailsClosedOnMissingCommandOrEvidence(t *testing.T) {
	catalog, err := semanticparity.Load("coverage.json")
	if err != nil {
		t.Fatal(err)
	}
	root := filepath.Clean(filepath.Join("..", ".."))
	commands, err := semanticparity.DiscoverCommands(context.Background())
	if err != nil {
		t.Fatal(err)
	}

	missingCommand := catalog
	missingCommand.Cases = append([]semanticparity.Case(nil), catalog.Cases...)
	missingCommand.Cases[0].Current = []string{"removed command"}
	if _, err := semanticparity.Validate(missingCommand, root, commands); err == nil || !strings.Contains(err.Error(), "missing evidence") {
		t.Fatalf("missing command must fail closed, got %v", err)
	}

	missingEvidence := catalog
	missingEvidence.Cases = append([]semanticparity.Case(nil), catalog.Cases...)
	missingEvidence.Cases[0].Evidence = []string{"tests/parity/does-not-exist.test"}
	if _, err := semanticparity.Validate(missingEvidence, root, commands); err == nil || !strings.Contains(err.Error(), "missing evidence") {
		t.Fatalf("missing evidence must fail closed, got %v", err)
	}
}

// normalize removes only documented volatile fields and canonicalizes object
// ordering. It deliberately preserves semantic arrays and all unknown fields.
func normalize(name string, value any) (any, bool) {
	obj, ok := value.(map[string]any)
	if !ok {
		return value, false
	}
	copy := make(map[string]any, len(obj))
	for k, v := range obj {
		copy[k] = v
	}
	switch name {
	case "remote-products-v1":
		delete(copy, "fetchedAt")
		delete(copy, "requestId")
	case "cache-state-v1":
		delete(copy, "measuredAt")
		delete(copy, "durationMs")
	case "profile-state-v1":
		delete(copy, "updatedAt")
		delete(copy, "revision")
	default:
		return value, false
	}
	for k, v := range copy {
		if child, ok := v.(map[string]any); ok {
			copy[k], _ = normalize(name, child)
		}
	}
	return copy, true
}

func TestSemanticNormalizersIgnoreOnlyDeclaredVolatility(t *testing.T) {
	cases := []struct{ name, left, right string }{
		{"remote-products-v1", `{"products":["wow","wow_classic"],"fetchedAt":"a","requestId":"1"}`, `{"requestId":"2","fetchedAt":"b","products":["wow","wow_classic"]}`},
		{"cache-state-v1", `{"bytes":12,"measuredAt":"a","durationMs":4}`, `{"durationMs":9,"measuredAt":"b","bytes":12}`},
		{"profile-state-v1", `{"name":"retail","build":"120100","updatedAt":"a","revision":1}`, `{"revision":2,"updatedAt":"b","build":"120100","name":"retail"}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var a, b any
			if err := json.Unmarshal([]byte(tc.left), &a); err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal([]byte(tc.right), &b); err != nil {
				t.Fatal(err)
			}
			na, ok := normalize(tc.name, a)
			if !ok {
				t.Fatal("normalizer rejected known family")
			}
			nb, _ := normalize(tc.name, b)
			ja, _ := json.Marshal(na)
			jb, _ := json.Marshal(nb)
			if !bytes.Equal(ja, jb) {
				t.Fatalf("semantically equal values differ:\n%s\n%s", ja, jb)
			}
			changed := strings.Replace(tc.right, `"120100"`, `"120101"`, 1)
			if tc.name != "profile-state-v1" {
				changed = strings.Replace(tc.right, `"wow_classic"`, `"wow_other"`, 1)
			}
			if tc.name == "cache-state-v1" {
				changed = strings.Replace(tc.right, `"bytes":12`, `"bytes":13`, 1)
			}
			var c any
			_ = json.Unmarshal([]byte(changed), &c)
			nc, _ := normalize(tc.name, c)
			jc, _ := json.Marshal(nc)
			if bytes.Equal(ja, jc) {
				t.Fatal("semantic field change was normalized away")
			}
		})
	}
}
