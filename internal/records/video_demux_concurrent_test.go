package records_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/video"
)

// Pause at the real public context boundary after the last frame is published
// and before the manifest, without adding a production-only test hook.
type demuxPublishPauseContext struct {
	context.Context
	lastFrame string
	published chan struct{}
	resume    chan struct{}
	once      sync.Once
}

func (c *demuxPublishPauseContext) Err() error {
	if _, err := os.Stat(c.lastFrame); err == nil {
		c.once.Do(func() {
			close(c.published)
			select {
			case <-c.resume:
			case <-c.Context.Done():
			}
		})
	}
	return c.Context.Err()
}

func TestDemuxAssetSerializesFramesAndManifestAcrossWorkspaces(t *testing.T) {
	aliases := []string{"normalized", "symlink"}
	if runtime.GOOS == "windows" {
		aliases = append(aliases, "case")
	}
	for _, aliasKind := range aliases {
		t.Run(aliasKind, func(t *testing.T) {
			rootA, rootB := workspaceRoot(t), workspaceRoot(t)
			parent := t.TempDir()
			output := filepath.Join(parent, "frames")
			if err := os.Mkdir(output, 0700); err != nil {
				t.Fatal(err)
			}
			alias := output + string(filepath.Separator) + "unused" + string(filepath.Separator) + ".."
			if aliasKind == "symlink" {
				alias = filepath.Join(parent, "alias")
				if err := os.Symlink(output, alias); err != nil {
					t.Skipf("creating a directory symlink is unavailable: %v", err)
				}
			} else if aliasKind == "case" {
				alias = filepath.Join(parent, "FRAMES")
			}
			parsed, err := video.ParseAVI(sampleAVI(t), video.Limits{})
			if err != nil {
				t.Fatal(err)
			}
			if len(parsed.Frames) != 2 || bytes.Equal(parsed.Frames[0].Payload, parsed.Frames[1].Payload) {
				t.Fatal("fixture must contain two different frame payloads")
			}
			sourceA, sourceB := filepath.Join(parent, "first.avi"), filepath.Join(parent, "second.avi")
			if err := os.WriteFile(sourceA, buildAVI(33333, 16, 16, [][]byte{parsed.Frames[0].Payload, parsed.Frames[1].Payload}), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(sourceB, buildAVI(33333, 16, 16, [][]byte{parsed.Frames[0].Payload, parsed.Frames[0].Payload}), 0600); err != nil {
				t.Fatal(err)
			}
			unrelated := []byte("caller-owned unrelated output")
			if err := os.WriteFile(filepath.Join(output, "keep.txt"), unrelated, 0600); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
			defer cancel()
			paused := &demuxPublishPauseContext{Context: ctx, lastFrame: filepath.Join(output, "frame-000002.00db"), published: make(chan struct{}), resume: make(chan struct{})}
			var release sync.Once
			defer release.Do(func() { close(paused.resume) })
			type outcome struct {
				result records.AssetDemux
				err    error
			}
			first := make(chan outcome, 1)
			firstFinished := false
			go func() {
				result, err := records.DemuxAsset(paused, rootA, "", records.AssetDemuxRequest{Path: sourceA, Output: output, Overwrite: true})
				first <- outcome{result, err}
			}()
			defer func() {
				release.Do(func() { close(paused.resume) })
				if !firstFinished {
					select {
					case <-first:
					case <-ctx.Done():
					}
				}
			}()
			select {
			case <-paused.published:
			case got := <-first:
				firstFinished = true
				t.Fatalf("export never reached frame/manifest boundary: %v", got.err)
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
			independentOutput := filepath.Join(parent, "independent")
			if err := os.Mkdir(independentOutput, 0700); err != nil {
				t.Fatal(err)
			}
			independentCtx, independentCancel := context.WithTimeout(ctx, 2*time.Second)
			independent, err := records.DemuxAsset(independentCtx, rootB, "", records.AssetDemuxRequest{Path: sourceB, Output: independentOutput})
			independentCancel()
			if err != nil {
				t.Fatalf("different output directory was blocked by pending manifest: %v", err)
			}
			checkDemuxCaptures(t, rootB, independent)

			cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestDemuxAssetExportHelperProcess$", "-test.v=false")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_DEMUX_HELPER=1", "LYCHEEDEV_DEMUX_ROOT="+rootB, "LYCHEEDEV_DEMUX_SOURCE="+sourceB, "LYCHEEDEV_DEMUX_OUTPUT="+alias)
			stdout, err := cmd.StdoutPipe()
			if err != nil {
				t.Fatal(err)
			}
			stdin, err := cmd.StdinPipe()
			if err != nil {
				t.Fatal(err)
			}
			defer stdin.Close()
			var stderr bytes.Buffer
			cmd.Stderr = &stderr
			if err := cmd.Start(); err != nil {
				t.Fatal(err)
			}
			stopped := false
			defer func() {
				if !stopped {
					_ = cmd.Process.Kill()
					_ = cmd.Wait()
				}
			}()
			reader := bufio.NewReader(stdout)
			readSignal := func(want string) {
				t.Helper()
				line, err := reader.ReadString('\n')
				if err != nil || strings.TrimSpace(line) != want {
					t.Fatalf("helper signal = %q, %v; want %q; %s", line, err, want, stderr.String())
				}
			}
			readSignal("blocked")
			// The other home/process cannot replace even one frame while the
			// first export's manifest is pending, and cancellation releases it.
			for i, frame := range parsed.Frames {
				raw, err := os.ReadFile(filepath.Join(output, fmt.Sprintf("frame-%06d.00db", i+1)))
				if err != nil || !bytes.Equal(raw, frame.Payload) {
					t.Fatalf("blocked exporter changed frame %d: %v", i, err)
				}
			}
			if _, err := os.Stat(filepath.Join(output, "demux-manifest.json")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("blocked exporter published a manifest: %v", err)
			}
			if _, err := fmt.Fprintln(stdin, "retry"); err != nil {
				t.Fatal(err)
			}
			// Start the successful second invocation while the first is still
			// paused. Both must finish with complete, separately verifiable sets.
			readSignal("started")
			release.Do(func() { close(paused.resume) })
			gotA := <-first
			firstFinished = true
			if gotA.err != nil {
				t.Fatal(gotA.err)
			}
			checkDemuxCaptures(t, rootA, gotA.result)
			line, err := reader.ReadString('\n')
			if err != nil {
				t.Fatal(err)
			}
			var gotB records.AssetDemux
			if err := json.Unmarshal([]byte(line), &gotB); err != nil {
				t.Fatalf("second result = %q: %v", line, err)
			}
			if err := cmd.Wait(); err != nil {
				stopped = true
				t.Fatalf("helper failed: %v; %s", err, stderr.String())
			}
			stopped = true
			checkDemuxCaptures(t, rootB, gotB)
			manifestRaw, err := os.ReadFile(filepath.Join(output, "demux-manifest.json"))
			if err != nil {
				t.Fatal(err)
			}
			wantManifest, err := json.MarshalIndent(gotB.Manifest, "", "  ")
			if err != nil || !bytes.Equal(manifestRaw, wantManifest) {
				t.Fatalf("published manifest is not the second export's completion record: %v", err)
			}
			for i, frame := range gotB.Manifest.Frames {
				raw, err := os.ReadFile(filepath.Join(output, frame.Output))
				if err != nil || sha256hex(raw) != frame.SHA256 || !bytes.Equal(raw, parsed.Frames[0].Payload) {
					t.Fatalf("published frame %d differs from its manifest/capture: %v", i, err)
				}
			}
			if gotA.result.Manifest.Source.SHA256 == gotB.Manifest.Source.SHA256 || gotA.result.Manifest.Frames[1].SHA256 == gotB.Manifest.Frames[1].SHA256 {
				t.Fatal("distinct source/frame identities were lost")
			}
			if raw, err := os.ReadFile(filepath.Join(output, "keep.txt")); err != nil || !bytes.Equal(raw, unrelated) {
				t.Fatalf("unrelated output changed: %v", err)
			}
			if files := publishedFiles(t, output); len(files) != 4 {
				t.Fatalf("export directory contains unexpected files: %v", keysOf(files))
			}
		})
	}
}

// The first Err call proves the public operation has begun before the parent
// releases its paused exporter; no timing sleep decides the successful order.
type demuxStartedContext struct {
	context.Context
	once sync.Once
}

func (c *demuxStartedContext) Err() error {
	c.once.Do(func() { fmt.Fprintln(os.Stdout, "started") })
	return c.Context.Err()
}

func TestDemuxAssetExportHelperProcess(t *testing.T) {
	if os.Getenv("LYCHEEDEV_DEMUX_HELPER") != "1" {
		return
	}
	root := os.Getenv("LYCHEEDEV_DEMUX_ROOT")
	request := records.AssetDemuxRequest{Path: os.Getenv("LYCHEEDEV_DEMUX_SOURCE"), Output: os.Getenv("LYCHEEDEV_DEMUX_OUTPUT"), Overwrite: true}
	ctx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	_, err := records.DemuxAsset(ctx, root, "", request)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("competing export returned %v, want deadline while the first manifest is pending", err)
	}
	fmt.Fprintln(os.Stdout, "blocked")
	if _, err := bufio.NewReader(os.Stdin).ReadString('\n'); err != nil {
		t.Fatal(err)
	}
	result, err := records.DemuxAsset(&demuxStartedContext{Context: context.Background()}, root, "", request)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		t.Fatal(err)
	}
}

func checkDemuxCaptures(t *testing.T, root string, result records.AssetDemux) {
	t.Helper()
	ctx := context.Background()
	manifest := result.Manifest
	if !manifest.Complete || manifest.Truncated || len(result.Frames) != len(manifest.Frames) || len(result.Published) != len(manifest.Frames)+1 {
		t.Fatalf("incomplete demux result: %+v", result)
	}
	archive := openEvidence(t, root)
	manifestRef, manifestRaw, err := archive.FetchCapture(ctx, result.Capture.ID, 1<<20)
	wantManifest, marshalErr := json.MarshalIndent(manifest, "", "  ")
	if err != nil || marshalErr != nil || !bytes.Equal(manifestRaw, wantManifest) || manifestRef.Blob != result.Capture.Blob || !manifestRef.Complete {
		t.Fatalf("manifest capture differs: %v, %v", err, marshalErr)
	}
	sourceRef, sourceRaw, err := archive.FetchCapture(ctx, manifest.SourceCapture, 1<<20)
	if err != nil || !reflect.DeepEqual(sourceRef, result.Source) || sha256hex(sourceRaw) != manifest.Source.SHA256 || sourceRef.Provenance.Kind != "asset-demux-source" {
		t.Fatalf("source capture differs: %v", err)
	}
	for i, frame := range manifest.Frames {
		ref, raw, err := archive.FetchCapture(ctx, frame.Capture, 1<<20)
		if err != nil || !reflect.DeepEqual(ref, result.Frames[i]) || ref.Blob != frame.Blob || sha256hex(raw) != frame.SHA256 || len(raw) != frame.Size || ref.Provenance.Kind != "asset-demux-frame" || ref.Provenance.Locator != fmt.Sprintf("%s#frame=%d", manifest.Input, i) {
			t.Fatalf("frame %d capture differs: %v", i, err)
		}
	}
}

// Observe real staging files at the existing cancellation boundaries. Every
// PublishBlob checks the context with its temporary file still present, so a
// second publication of identical bytes remains visible even when it coalesces
// into an existing content-addressed blob. No production counters are needed.
type demuxBlobStagesContext struct {
	context.Context
	tmp    string
	mu     sync.Mutex
	stages map[string]int64
}

func (c *demuxBlobStagesContext) Err() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	entries, _ := os.ReadDir(c.tmp)
	for _, entry := range entries {
		if !strings.HasPrefix(entry.Name(), "blob-") {
			continue
		}
		if info, err := entry.Info(); err == nil {
			c.stages[entry.Name()] = max(c.stages[entry.Name()], info.Size())
		}
	}
	return c.Context.Err()
}

func TestDemuxAssetStagesEachCaptureBlobOnce(t *testing.T) {
	root := workspaceRoot(t)
	output := t.TempDir()
	ctx := &demuxBlobStagesContext{Context: context.Background(), tmp: filepath.Join(root, "tmp"), stages: make(map[string]int64)}
	result, err := records.DemuxAsset(ctx, root, "", records.AssetDemuxRequest{Path: samplePath(t), Output: output})
	if err != nil {
		t.Fatal(err)
	}
	checkDemuxCaptures(t, root, result)
	wantBytes := result.Source.Blob.Bytes + result.Capture.Blob.Bytes
	for _, frame := range result.Frames {
		wantBytes += frame.Blob.Bytes
	}
	ctx.mu.Lock()
	defer ctx.mu.Unlock()
	var stagedBytes int64
	for _, size := range ctx.stages {
		stagedBytes += size
	}
	if len(ctx.stages) != len(result.Frames)+2 || stagedBytes != wantBytes {
		t.Fatalf("blob staging = %d files / %d bytes, want source + %d frames + manifest = %d files / %d bytes", len(ctx.stages), stagedBytes, len(result.Frames), len(result.Frames)+2, wantBytes)
	}
	t.Logf("%d frames: %d blob stages, %d bytes; exact source + frames + manifest budget", len(result.Frames), len(ctx.stages), stagedBytes)
}
