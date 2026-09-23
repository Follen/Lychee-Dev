package evidence

import (
	"archive/zip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrBundleSelection = errors.New("evidence.bundle_selection")
var ErrBundleOutput = errors.New("evidence.bundle_output")
var ErrBundleLimit = errors.New("evidence.bundle_limit")

type BundleManifest struct {
	Schema   string       `json:"schema"`
	Output   string       `json:"output"`
	Captures []CaptureRef `json:"captures"`
	Bytes    int64        `json:"bytes"`
	Complete bool         `json:"complete"`
}

// Bundle writes one atomic ZIP containing exact verified bytes and immutable
// capture manifests. It refuses any existing output and never reads legacy
// stores or follows output symlinks. The ZIP manifest is written last.
func Bundle(ctx context.Context, root string, ids []string, output string) (BundleManifest, error) {
	if len(ids) < 1 || len(ids) > 100 {
		return BundleManifest{}, ErrBundleSelection
	}
	seen := map[string]bool{}
	for _, id := range ids {
		if !strings.HasPrefix(id, "CAP-") || len(id) != 68 || seen[id] {
			return BundleManifest{}, ErrBundleSelection
		}
		seen[id] = true
	}
	if output == "" {
		return BundleManifest{}, ErrBundleOutput
	}
	abs, err := filepath.Abs(output)
	if err != nil {
		return BundleManifest{}, err
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(abs))
	if err != nil {
		return BundleManifest{}, fmt.Errorf("%w: %v", ErrBundleOutput, err)
	}
	abs = filepath.Join(parent, filepath.Base(abs))
	workspace, err := filepath.EvalSymlinks(root)
	if err != nil {
		return BundleManifest{}, err
	}
	workspace, err = filepath.Abs(workspace)
	if err != nil {
		return BundleManifest{}, err
	}
	if relative, err := filepath.Rel(workspace, abs); err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return BundleManifest{}, ErrBundleOutput
	}
	if _, err := os.Lstat(abs); err == nil {
		return BundleManifest{}, ErrBundleOutput
	} else if !errors.Is(err, os.ErrNotExist) {
		return BundleManifest{}, err
	}
	stage, err := os.CreateTemp(parent, ".lycheedev-bundle-")
	if err != nil {
		return BundleManifest{}, err
	}
	defer os.Remove(stage.Name())
	defer stage.Close()
	manifest := BundleManifest{Schema: "lycheedev.evidence-bundle.v1", Output: abs, Captures: make([]CaptureRef, 0, len(ids)), Complete: true}
	writer := zip.NewWriter(stage)
	usedObjects := map[string]bool{}
	_, err = vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (bool, error) {
		archive := OpenArchive(s, m)
		for _, id := range ids {
			if err := ctx.Err(); err != nil {
				return false, err
			}
			ref, err := archive.InspectCapture(ctx, id)
			if err != nil {
				return false, err
			}
			if ref.Blob.Bytes > 32<<20 || !usedObjects[ref.Blob.SHA256] && manifest.Bytes+ref.Blob.Bytes > 128<<20 {
				return false, ErrBundleLimit
			}
			raw, err := s.ReadBlob(ctx, ref.Blob, 32<<20)
			if err != nil {
				return false, err
			}
			if !usedObjects[ref.Blob.SHA256] {
				entry, err := writer.Create("objects/" + ref.Blob.SHA256)
				if err != nil {
					return false, err
				}
				if _, err := entry.Write(raw); err != nil {
					return false, err
				}
				usedObjects[ref.Blob.SHA256] = true
				manifest.Bytes += int64(len(raw))
			}
			encoded, err := json.Marshal(ref)
			if err != nil {
				return false, err
			}
			entry, err := writer.Create("captures/" + id + ".json")
			if err != nil {
				return false, err
			}
			if _, err := entry.Write(encoded); err != nil {
				return false, err
			}
			manifest.Captures = append(manifest.Captures, ref)
		}
		return true, nil
	})
	if err != nil {
		writer.Close()
		return BundleManifest{}, err
	}
	encoded, err := json.Marshal(manifest)
	if err != nil {
		writer.Close()
		return BundleManifest{}, err
	}
	entry, err := writer.Create("manifest.json")
	if err != nil {
		writer.Close()
		return BundleManifest{}, err
	}
	if _, err := entry.Write(encoded); err != nil {
		writer.Close()
		return BundleManifest{}, err
	}
	if err := writer.Close(); err != nil {
		return BundleManifest{}, err
	}
	if err := stage.Sync(); err != nil {
		return BundleManifest{}, err
	}
	if err := stage.Close(); err != nil {
		return BundleManifest{}, err
	}
	if err := os.Link(stage.Name(), abs); err != nil {
		if errors.Is(err, os.ErrExist) {
			return BundleManifest{}, ErrBundleOutput
		}
		return BundleManifest{}, err
	}
	return manifest, nil
}
