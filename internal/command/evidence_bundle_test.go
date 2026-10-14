package command

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync/atomic"
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

// Observe the completed ZIP stage at the public context boundary, not elapsed
// time or a production hook. The original implementation never checked the
// context after the last blob read, so it published without observing this.
type bundlePublicationContext struct {
	context.Context
	directory string
	cancel    context.CancelFunc
	observed  atomic.Bool
}

func (c *bundlePublicationContext) Err() error {
	if !c.observed.Load() {
		stages, _ := filepath.Glob(filepath.Join(c.directory, ".lycheedev-bundle-*"))
		for _, stage := range stages {
			reader, err := zip.OpenReader(stage)
			if err != nil {
				continue
			}
			complete := len(reader.File) > 0 && reader.File[len(reader.File)-1].Name == "manifest.json"
			reader.Close()
			if complete {
				c.observed.Store(true)
				c.cancel()
				break
			}
		}
	}
	return c.Context.Err()
}

func TestEvidenceBundleCancellationBeforePublication(t *testing.T) {
	base := context.Background()
	root := t.TempDir()
	if _, err := vault.Initialize(base, root); err != nil {
		t.Fatal(err)
	}
	ref, err := vault.WriteMetadata(base, root, func(s *vault.Store, m *vault.Metadata) (evidence.CaptureRef, error) {
		return evidence.OpenArchive(s, m).CommitCapture(base, evidence.CaptureDraft{
			Reader: bytes.NewReader([]byte("exact bytes after cancellation")), MaxBytes: 1024,
			MediaType: "text/plain", Complete: true, Provenance: evidence.Provenance{Kind: "fixture", Locator: "bundle-cancel"},
		})
	})
	if err != nil {
		t.Fatal(err)
	}
	parent := t.TempDir()
	output := filepath.Join(parent, "report.zip")
	ctx, cancel := context.WithCancel(base)
	defer cancel()
	boundary := &bundlePublicationContext{Context: ctx, directory: parent, cancel: cancel}
	args := []string{"evidence", "bundle", "--ids", ref.ID, "--output", output, "--home", root, "--format=json"}
	var stdout, stderr bytes.Buffer
	code := Execute(boundary, args, &stdout, &stderr)
	var response Envelope
	if err := json.Unmarshal(stdout.Bytes(), &response); err != nil {
		t.Fatalf("response: %v stdout=%s stderr=%s", err, stdout.Bytes(), stderr.Bytes())
	}
	if !boundary.observed.Load() || !errors.Is(ctx.Err(), context.Canceled) || code != 7 || response.OK || response.Error == nil || response.Error.Code != "command.cancelled" {
		t.Fatalf("late cancellation: observed=%v context=%v code=%d response=%+v", boundary.observed.Load(), ctx.Err(), code, response)
	}
	if _, err := os.Stat(output); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("cancelled bundle published output: %v", err)
	}
	if stages, err := filepath.Glob(filepath.Join(parent, ".lycheedev-bundle-*")); err != nil || len(stages) != 0 {
		t.Fatalf("cancelled bundle retained staging files: %v %v", stages, err)
	}
	// No public output was committed: the same selection and output remain
	// usable for an explicit later invocation with a fresh caller context.
	retried, code := invoke(t, args...)
	if code != 0 || !retried.OK || resultMap(t, retried)["complete"] != true {
		t.Fatalf("retry after cancelled staging: code=%d response=%+v", code, retried)
	}
}
