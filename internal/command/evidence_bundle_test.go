package command

import (
	"archive/zip"
	"bytes"
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestEvidenceBundleCLI(t *testing.T) {
	ctx := context.Background()
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, workspace); err != nil {
		t.Fatal(err)
	}
	ref, err := vault.WriteMetadata(ctx, workspace, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader([]byte("exact bytes")), MaxBytes: 100,
			MediaType: "text/plain", Complete: true, Provenance: evidence.Provenance{Kind: "fixture", Locator: "fixture"}})
	})
	if err != nil {
		t.Fatal(err)
	}
	output := filepath.Join(t.TempDir(), "report.zip")
	result, code := invoke(t, "evidence", "bundle", "--ids", ref.ID, "--output", output, "--home", workspace, "--format=json")
	if code != 0 || !result.OK || resultMap(t, result)["complete"] != true {
		t.Fatalf("bundle: %+v (%d)", result, code)
	}
	file, err := zip.OpenReader(output)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if len(file.File) != 3 || file.File[2].Name != "manifest.json" {
		t.Fatalf("bundle members: %+v", file.File)
	}
	conflict, code := invoke(t, "evidence", "bundle", "--ids", ref.ID, "--output", output, "--home", workspace, "--format=json")
	if code != 2 || conflict.OK || conflict.Error.Code != "evidence.bundle_output" {
		t.Fatalf("existing output accepted: %+v (%d)", conflict, code)
	}
	if _, err := os.Stat(output); err != nil {
		t.Fatal(err)
	}
}
