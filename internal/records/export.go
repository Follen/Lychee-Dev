package records

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

type AssetExportRequest struct {
	File      FileQuery
	Output    string
	Overwrite bool
	Encoding  string
	Mipmap    int
	Channels  string
	MaxPixels int64
}

type AssetExportManifest struct {
	Schema          string               `json:"schema"`
	Snapshot        string               `json:"snapshot"`
	Encoding        string               `json:"encoding"`
	Output          string               `json:"output"`
	Content         vault.BlobRef        `json:"content"`
	Source          FileReading          `json:"source"`
	SourceCapture   evidence.CaptureRef  `json:"sourceCapture"`
	Image           *ImageExport         `json:"image,omitempty"`
	ArtifactCapture *evidence.CaptureRef `json:"artifactCapture,omitempty"`
}

type AssetExport struct {
	Manifest AssetExportManifest
	Capture  evidence.CaptureRef
	Artifact evidence.CaptureRef
}

// ExportAsset prepares the full verified source and a reproducible manifest
// before publishing one complete output file. Captures certify bytes/provenance,
// not the continued existence of an external path. Only a successful return
// acknowledges publication; cancellation or a conflict never truncates a file.
func ExportAsset(ctx context.Context, root, snapshot string, request AssetExportRequest) (AssetExport, error) {
	if err := ctx.Err(); err != nil {
		return AssetExport{}, err
	}
	request, err := normalizeAssetExport(request)
	if err != nil {
		return AssetExport{}, err
	}
	dst, err := openExportDestination(request.Output, request.Overwrite)
	if err != nil {
		return AssetExport{}, err
	}
	defer dst.Close()
	// Exports must not replace the workspace's own identities, evidence or locks.
	workspace, err := filepath.EvalSymlinks(root)
	if err != nil {
		return AssetExport{}, err
	}
	workspace, err = filepath.Abs(workspace)
	if err != nil {
		return AssetExport{}, err
	}
	if relative, err := filepath.Rel(workspace, dst.path); err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return AssetExport{}, fmt.Errorf("%w: output must be outside the managed workspace", ErrExportPath)
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (AssetExport, error) {
		reading, raw, err := captureAsset(ctx, s, m, snapshot, request.File)
		if err != nil {
			return AssetExport{}, err
		}
		artifact := reading.Capture
		var imageInfo *ImageExport
		if request.Encoding != "raw" {
			raw, imageInfo, err = encodeAssetImage(ctx, raw, request)
			if err != nil {
				return AssetExport{}, err
			}
			artifact, err = evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
				Reader: bytes.NewReader(raw), MaxBytes: request.File.ContentBytes, MediaType: "image/" + request.Encoding, Complete: true,
				Provenance: evidence.Provenance{Kind: "asset-image", Locator: reading.Capture.ID, Snapshot: snapshot, DataBuild: reading.Result.Pin.FullBuild},
			})
			if err != nil {
				return AssetExport{}, err
			}
		}
		manifest := AssetExportManifest{
			Schema: "lycheedev.asset-export.v1", Snapshot: snapshot, Encoding: request.Encoding, Output: dst.path,
			Content: artifact.Blob, Source: reading.Result, SourceCapture: reading.Capture, Image: imageInfo,
		}
		if request.Encoding != "raw" {
			manifest.ArtifactCapture = &artifact
		}
		encoded, err := json.Marshal(manifest)
		if err != nil {
			return AssetExport{}, err
		}
		capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(encoded), MaxBytes: 1 << 20, MediaType: "application/json", Complete: true,
			Provenance: evidence.Provenance{Kind: "asset-export-manifest", Locator: reading.Capture.ID, Snapshot: snapshot, DataBuild: reading.Result.Pin.FullBuild},
		})
		if err != nil {
			return AssetExport{}, err
		}
		if err := dst.publish(ctx, raw); err != nil {
			return AssetExport{}, err
		}
		return AssetExport{Manifest: manifest, Capture: capture, Artifact: artifact}, nil
	})
}
