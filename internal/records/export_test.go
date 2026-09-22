package records_test

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestExportAssetEvidenceAndFailurePreservation(t *testing.T) {
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(ctx, workspace)
	if err != nil {
		t.Fatal(err)
	}
	body := []byte("complete asset\x00\xff\r\n")
	pin := testkit.CachedAsset(t, workspace, body)
	output := filepath.Join(t.TempDir(), "asset.bin")
	query := records.FileQuery{CDN: true, Offline: true, FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1 << 20}
	request := records.AssetExportRequest{File: query, Output: output}
	exported, err := records.ExportAsset(ctx, workspace, pin.ID, request)
	if err != nil {
		t.Fatal(err)
	}
	m, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	archive := evidence.OpenArchive(store, m)
	ref, raw, err := archive.FetchCapture(ctx, exported.Capture.ID, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	var manifest records.AssetExportManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	if ref.Provenance.Kind != "asset-export-manifest" || ref.Provenance.Locator != manifest.SourceCapture.ID || manifest.Snapshot != pin.ID || manifest.Source.Entry.FileDataID != 11 || manifest.Source.Pin != *pin.Data || manifest.Content != manifest.Source.Content || manifest.Content != manifest.SourceCapture.Blob {
		t.Fatalf("wrong provenance: %+v", manifest)
	}
	_, captured, err := archive.FetchCapture(ctx, manifest.SourceCapture.ID, 1<<20)
	if err != nil || !bytes.Equal(captured, body) {
		t.Fatalf("source bytes: %v", err)
	}
	request.Overwrite = true
	request.File.FileDataID = 12
	if _, err := records.ExportAsset(ctx, workspace, pin.ID, request); !errors.Is(err, records.ErrContentMissing) {
		t.Fatalf("missing file: %v", err)
	}
	got, err := os.ReadFile(output)
	if err != nil || !bytes.Equal(got, body) {
		t.Fatal("missing file replaced output", err)
	}
	request.File = query
	// Corruption of a cached configuration must fail before touching output.
	config := manifest.Source.BuildConfiguration
	if err := os.WriteFile(filepath.Join(workspace, "blobs", config.SHA256[:2], config.SHA256[2:]), []byte("corrupt"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := records.ExportAsset(ctx, workspace, pin.ID, request); !errors.Is(err, vault.ErrBlobIntegrity) {
		t.Fatalf("corrupt source: %v", err)
	}
	got, err = os.ReadFile(output)
	if err != nil || !bytes.Equal(got, body) {
		t.Fatal("corrupt source replaced output", err)
	}
}

func TestExportAssetRefusesManagedWorkspace(t *testing.T) {
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	identityPath := filepath.Join(workspace, "workspace.json")
	want, err := os.ReadFile(identityPath)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{identityPath, filepath.Join(workspace, "new-file")} {
		_, err := records.ExportAsset(ctx, workspace, "unused", records.AssetExportRequest{Output: path, Overwrite: true})
		if !errors.Is(err, records.ErrExportPath) {
			t.Fatalf("%s: %v", path, err)
		}
	}
	got, err := os.ReadFile(identityPath)
	if err != nil || !bytes.Equal(got, want) {
		t.Fatal("workspace identity replaced", err)
	}
}

func TestExportAssetConflictPrecedesSourceAccess(t *testing.T) {
	output := filepath.Join(t.TempDir(), "keep.bin")
	if err := os.WriteFile(output, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	_, err := records.ExportAsset(context.Background(), "must-not-open", "unused", records.AssetExportRequest{Output: output})
	if !errors.Is(err, records.ErrExportConflict) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = records.ExportAsset(ctx, "must-not-open", "unused", records.AssetExportRequest{})
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestExportAssetEmptyContent(t *testing.T) {
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	pin := testkit.CachedAsset(t, workspace, nil)
	output := filepath.Join(t.TempDir(), "empty.bin")
	exported, err := records.ExportAsset(ctx, workspace, pin.ID, records.AssetExportRequest{
		Output: output, File: records.FileQuery{CDN: true, Offline: true, FileDataID: 11, MetadataBytes: 1 << 20, ContentBytes: 1024},
	})
	if err != nil {
		t.Fatal(err)
	}
	if exported.Manifest.Content.Bytes != 0 || exported.Manifest.Content.SHA256 != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
		t.Fatal(exported)
	}
	got, err := os.ReadFile(output)
	if err != nil || len(got) != 0 {
		t.Fatalf("empty output: %x %v", got, err)
	}
}
