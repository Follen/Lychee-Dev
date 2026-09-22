// SPDX-License-Identifier: AGPL-3.0-or-later
// Video demux use case built on wowdata's AVI semantics; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/video"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// AssetDemuxSchema versions the machine-readable demux manifest.
const AssetDemuxSchema = "lycheedev.asset-demux.v1"

const assetDemuxManifestName = "demux-manifest.json"

var ErrDemuxInputLimit = errors.New("records.demux_input_limit")

// ErrFrameLimit is the module-visible form of the video bounds error
// (records.frame_limit_exceeded) raised when a frame or frame sequence
// violates the declared demux limits.
var ErrFrameLimit = video.ErrFrameLimit

// AssetDemuxRequest demuxes one VP9-in-AVI container from either the pinned
// CASC source (File.FileDataID) or a local file (Path). Output is an existing
// directory outside the managed workspace; published files refuse to replace
// existing files unless Overwrite is set.
type AssetDemuxRequest struct {
	File           FileQuery
	Path           string
	Output         string
	Overwrite      bool
	MaxInputBytes  int64
	MaxFrames      int
	MaxFrameBytes  int64
	MaxOutputBytes int64
	AllowPartial   bool
}

// AssetDemuxSource fixes the demuxed input identity: snapshot/build/file ID
// for CASC inputs, canonical path and content digest for local inputs.
type AssetDemuxSource struct {
	Kind        string             `json:"kind"`
	Input       string             `json:"input"`
	Path        string             `json:"path,omitempty"`
	FileDataID  uint32             `json:"fileDataID,omitempty"`
	Snapshot    string             `json:"snapshot,omitempty"`
	Pin         *selection.DataPin `json:"pin,omitempty"`
	ContentKey  string             `json:"contentKey,omitempty"`
	EncodingKey string             `json:"encodingKey,omitempty"`
	Bytes       int64              `json:"bytes"`
	SHA256      string             `json:"sha256"`
}

// AssetDemuxFrame is one published frame record. The type/timestamp/duration/
// size fields keep the legacy metadata semantics; output, digest, blob and
// capture make the payload verifiable.
type AssetDemuxFrame struct {
	Index     int           `json:"index"`
	Type      string        `json:"type"`
	Timestamp float64       `json:"timestamp"`
	Duration  float64       `json:"duration"`
	Size      int           `json:"size"`
	Chunk     string        `json:"chunk"`
	Offset    int64         `json:"offset"`
	Output    string        `json:"output"`
	SHA256    string        `json:"sha256"`
	Blob      vault.BlobRef `json:"blob"`
	Capture   string        `json:"capture"`
	SubFrames []int         `json:"subFrames,omitempty"`
}

// AssetDemuxLimits records the visible bounds of one demux run.
type AssetDemuxLimits struct {
	MaxInputBytes  int64 `json:"maxInputBytes"`
	MaxFrames      int   `json:"maxFrames"`
	MaxFrameBytes  int64 `json:"maxFrameBytes"`
	MaxOutputBytes int64 `json:"maxOutputBytes"`
}

// AssetDemuxManifest is the reproducible record of one demux run.
type AssetDemuxManifest struct {
	Schema           string            `json:"schema"`
	Snapshot         string            `json:"snapshot,omitempty"`
	Input            string            `json:"input"`
	Source           AssetDemuxSource  `json:"source"`
	Output           string            `json:"output"`
	Width            int               `json:"width"`
	Height           int               `json:"height"`
	FrameRate        float64           `json:"frameRate"`
	FrameCount       int               `json:"frameCount"`
	EnumeratedFrames int               `json:"enumeratedFrames"`
	EnumeratedBytes  int64             `json:"enumeratedBytes"`
	Frames           []AssetDemuxFrame `json:"frames"`
	Complete         bool              `json:"complete"`
	Truncated        bool              `json:"truncated"`
	TruncationReason string            `json:"truncationReason,omitempty"`
	Warnings         []string          `json:"warnings,omitempty"`
	Limits           AssetDemuxLimits  `json:"limits"`
	SourceCapture    string            `json:"sourceCapture"`
}

// AssetDemux is one demux result: the manifest, its evidence captures and the
// exact external files that were published. Only a successful return
// acknowledges the complete publication; the manifest file is published last
// and is the completion record of the artifact set.
type AssetDemux struct {
	Manifest  AssetDemuxManifest
	Source    evidence.CaptureRef
	Frames    []evidence.CaptureRef
	Capture   evidence.CaptureRef
	Published []string
}

const (
	demuxDefaultMaxInput  = 256 << 20
	demuxDefaultMaxFrames = 100000
	demuxDefaultMaxFrame  = 256 << 20
	demuxDefaultMaxOutput = 1 << 30
)

func normalizeAssetDemux(r AssetDemuxRequest) (AssetDemuxRequest, error) {
	if (r.Path == "") == (r.File.FileDataID == 0) {
		return r, ErrFileQuery
	}
	if r.Output == "" {
		return r, ErrExportPath
	}
	if r.MaxInputBytes == 0 {
		r.MaxInputBytes = demuxDefaultMaxInput
	}
	if r.MaxFrames == 0 {
		r.MaxFrames = demuxDefaultMaxFrames
	}
	if r.MaxFrameBytes == 0 {
		r.MaxFrameBytes = demuxDefaultMaxFrame
	}
	if r.MaxOutputBytes == 0 {
		r.MaxOutputBytes = demuxDefaultMaxOutput
	}
	if r.MaxInputBytes < 12 || r.MaxInputBytes > 1<<40 ||
		r.MaxFrames < 1 || r.MaxFrames > 1<<24 ||
		r.MaxFrameBytes < 1 || r.MaxFrameBytes > 1<<40 ||
		r.MaxOutputBytes < 1 || r.MaxOutputBytes > 1<<40 {
		return r, ErrDemuxInputLimit
	}
	if r.Path != "" {
		if r.File.Installation != "" || r.File.CDN {
			return r, ErrFileQuery
		}
	} else {
		if r.File.MetadataBytes == 0 {
			r.File.MetadataBytes = 64 << 20
		}
		if r.File.ContentBytes == 0 {
			r.File.ContentBytes = r.MaxInputBytes
		}
	}
	return r, nil
}

// DemuxAsset is the video demux capability: bounded VP9-in-AVI frame
// extraction with legacy frame metadata, per-frame payload files, content
// hashes and a lycheedev.asset-demux.v1 manifest, all through the shared
// stage-then-publish export chain and the evidence archive.
func DemuxAsset(ctx context.Context, root, snapshot string, request AssetDemuxRequest) (AssetDemux, error) {
	if err := ctx.Err(); err != nil {
		return AssetDemux{}, err
	}
	request, err := normalizeAssetDemux(request)
	if err != nil {
		return AssetDemux{}, err
	}
	if request.Path == "" && snapshot == "" {
		// A CASC read must run against a fixed snapshot identity.
		return AssetDemux{}, ErrSnapshotRequired
	}
	if err := checkDemuxOutput(root, request.Output); err != nil {
		return AssetDemux{}, err
	}
	manifestPath := filepath.Join(request.Output, assetDemuxManifestName)
	// A complete previous run publishes its manifest first on re-runs: refuse
	// (or accept with --overwrite) before reading any source bytes.
	if probe, err := openExportDestination(manifestPath, request.Overwrite); err != nil {
		return AssetDemux{}, err
	} else {
		_ = probe.Close()
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (AssetDemux, error) {
		return demuxAsset(ctx, s, m, root, snapshot, request, manifestPath)
	})
}

func demuxAsset(ctx context.Context, s *vault.Store, m *vault.Metadata, root, snapshot string, request AssetDemuxRequest, manifestPath string) (AssetDemux, error) {
	raw, source, sourceCapture, err := demuxSource(ctx, s, m, snapshot, request)
	if err != nil {
		return AssetDemux{}, err
	}
	parsed, err := video.ParseAVI(raw, video.Limits{
		MaxFrames:     request.MaxFrames,
		MaxFrameBytes: request.MaxFrameBytes,
		MaxTotalBytes: request.MaxOutputBytes,
		AllowPartial:  request.AllowPartial,
	})
	if err != nil {
		return AssetDemux{}, err
	}
	if err := ctx.Err(); err != nil {
		return AssetDemux{}, err
	}
	// Preflight every output name before any external write so a name conflict
	// cannot leave a half-published artifact set.
	outputs := make([]string, 0, len(parsed.Frames)+1)
	for index, frame := range parsed.Frames {
		outputs = append(outputs, fmt.Sprintf("frame-%06d.%s", index+1, frame.Chunk))
	}
	outputs = append(outputs, assetDemuxManifestName)
	destinations := make([]*exportDestination, 0, len(outputs))
	defer func() {
		for _, destination := range destinations {
			_ = destination.Close()
		}
	}()
	for _, name := range outputs {
		destination, err := openExportDestination(filepath.Join(request.Output, name), request.Overwrite)
		if err != nil {
			return AssetDemux{}, err
		}
		destinations = append(destinations, destination)
	}
	manifest := AssetDemuxManifest{
		Schema:           AssetDemuxSchema,
		Snapshot:         snapshot,
		Source:           source,
		Output:           request.Output,
		Width:            parsed.Width,
		Height:           parsed.Height,
		FrameRate:        parsed.FrameRate,
		FrameCount:       len(parsed.Frames),
		EnumeratedFrames: parsed.EnumeratedFrames,
		EnumeratedBytes:  parsed.EnumeratedBytes,
		Frames:           make([]AssetDemuxFrame, 0, len(parsed.Frames)),
		Complete:         parsed.Complete,
		Truncated:        parsed.Truncated,
		TruncationReason: parsed.TruncationReason,
		Warnings:         parsed.Warnings,
		Limits: AssetDemuxLimits{
			MaxInputBytes: request.MaxInputBytes, MaxFrames: request.MaxFrames,
			MaxFrameBytes: request.MaxFrameBytes, MaxOutputBytes: request.MaxOutputBytes,
		},
		SourceCapture: sourceCapture.ID,
	}
	if request.Path != "" {
		manifest.Input = request.Path
	} else {
		manifest.Input = fmt.Sprintf("casc:fdid:%d", request.File.FileDataID)
	}
	demux := AssetDemux{Source: sourceCapture}
	build := ""
	if source.Pin != nil {
		build = source.Pin.FullBuild
	}
	for index, frame := range parsed.Frames {
		if err := ctx.Err(); err != nil {
			return AssetDemux{}, err
		}
		digest := sha256.Sum256(frame.Payload)
		blob, err := s.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(frame.Payload), MaxBytes: request.MaxOutputBytes})
		if err != nil {
			return AssetDemux{}, err
		}
		capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(frame.Payload), MaxBytes: request.MaxFrameBytes, MediaType: "video/VP9", Complete: true,
			Provenance: evidence.Provenance{
				Kind: "asset-demux-frame", Locator: fmt.Sprintf("%s#frame=%d", manifest.Input, index),
				Snapshot: snapshot, DataBuild: build,
			},
		})
		if err != nil {
			return AssetDemux{}, err
		}
		demux.Frames = append(demux.Frames, capture)
		manifest.Frames = append(manifest.Frames, AssetDemuxFrame{
			Index: index, Type: frame.Type, Timestamp: frame.Timestamp, Duration: frame.Duration,
			Size: frame.Size, Chunk: frame.Chunk, Offset: frame.Offset,
			Output: outputs[index], SHA256: hex.EncodeToString(digest[:]),
			Blob: blob, Capture: capture.ID, SubFrames: frame.SubFrames,
		})
	}
	encoded, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return AssetDemux{}, err
	}
	capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
		Reader: bytes.NewReader(encoded), MaxBytes: 16 << 20, MediaType: "application/json", Complete: manifest.Complete,
		Truncated: manifest.Truncated,
		Provenance: evidence.Provenance{
			Kind: "asset-demux-manifest", Locator: manifest.Input, Snapshot: snapshot, DataBuild: build,
		},
	})
	if err != nil {
		return AssetDemux{}, err
	}
	demux.Capture = capture
	// Frames publish first and the manifest last: without a successful return
	// and a manifest file the artifact set is visibly incomplete.
	for index, frame := range parsed.Frames {
		if err := ctx.Err(); err != nil {
			return AssetDemux{}, err
		}
		if err := destinations[index].publish(ctx, frame.Payload); err != nil {
			return AssetDemux{}, err
		}
		demux.Published = append(demux.Published, destinations[index].path)
	}
	if err := destinations[len(destinations)-1].publish(ctx, encoded); err != nil {
		return AssetDemux{}, err
	}
	demux.Published = append(demux.Published, manifestPath)
	demux.Manifest = manifest
	return demux, nil
}

// demuxSource reads and evidences the full input container from the selected
// source: the shared CASC reader chain or one bounded local file.
func demuxSource(ctx context.Context, s *vault.Store, m *vault.Metadata, snapshot string, request AssetDemuxRequest) ([]byte, AssetDemuxSource, evidence.CaptureRef, error) {
	if request.Path != "" {
		raw, canonical, err := readLocalInput(ctx, request.Path, request.MaxInputBytes)
		if err != nil {
			return nil, AssetDemuxSource{}, evidence.CaptureRef{}, err
		}
		digest := sha256.Sum256(raw)
		source := AssetDemuxSource{
			Kind: "file", Input: request.Path, Path: canonical,
			Bytes: int64(len(raw)), SHA256: hex.EncodeToString(digest[:]),
		}
		if snapshot != "" {
			source.Snapshot = snapshot
		}
		capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(raw), MaxBytes: request.MaxInputBytes, MediaType: "video/x-msvideo", Complete: true,
			Provenance: evidence.Provenance{Kind: "asset-demux-source", Locator: "file:" + canonical, Snapshot: snapshot},
		})
		if err != nil {
			return nil, AssetDemuxSource{}, evidence.CaptureRef{}, err
		}
		return raw, source, capture, nil
	}
	reading, raw, err := captureAsset(ctx, s, m, snapshot, request.File)
	if err != nil {
		return nil, AssetDemuxSource{}, evidence.CaptureRef{}, err
	}
	if int64(len(raw)) > request.MaxInputBytes {
		return nil, AssetDemuxSource{}, evidence.CaptureRef{}, ErrDemuxInputLimit
	}
	digest := sha256.Sum256(raw)
	pin := reading.Result.Pin
	source := AssetDemuxSource{
		Kind: "casc", Input: fmt.Sprintf("casc:fdid:%d", request.File.FileDataID),
		FileDataID: request.File.FileDataID, Snapshot: snapshot, Pin: &pin,
		ContentKey: reading.Result.Entry.ContentKey, EncodingKey: reading.Result.PayloadEncodingKey,
		Bytes: int64(len(raw)), SHA256: hex.EncodeToString(digest[:]),
	}
	return raw, source, reading.Capture, nil
}

func readLocalInput(ctx context.Context, path string, maxBytes int64) ([]byte, string, error) {
	if err := ctx.Err(); err != nil {
		return nil, "", err
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, "", ErrFileQuery
	}
	canonical, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return nil, "", err
	}
	file, err := os.Open(canonical)
	if err != nil {
		return nil, "", err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, "", err
	}
	if !info.Mode().IsRegular() {
		return nil, "", ErrFileQuery
	}
	if info.Size() > maxBytes {
		return nil, "", ErrDemuxInputLimit
	}
	raw, err := io.ReadAll(io.LimitReader(&demuxContextReader{ctx: ctx, source: file}, maxBytes+1))
	if err != nil {
		return nil, "", err
	}
	if int64(len(raw)) > maxBytes {
		return nil, "", ErrDemuxInputLimit
	}
	return raw, canonical, nil
}

type demuxContextReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *demuxContextReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.source.Read(p)
}

// checkDemuxOutput enforces the export contract for the output directory: it
// must already exist and stay outside the managed workspace.
func checkDemuxOutput(root, output string) error {
	abs, err := filepath.Abs(output)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrExportPath, err)
	}
	info, err := os.Stat(abs)
	if err != nil || !info.IsDir() {
		return fmt.Errorf("%w: %s must be an existing directory", ErrExportPath, output)
	}
	resolved, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrExportPath, err)
	}
	workspace, err := filepath.EvalSymlinks(root)
	if err != nil {
		return err
	}
	workspace, err = filepath.Abs(workspace)
	if err != nil {
		return err
	}
	if relative, err := filepath.Rel(workspace, resolved); err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return fmt.Errorf("%w: output must be outside the managed workspace", ErrExportPath)
	}
	return nil
}
