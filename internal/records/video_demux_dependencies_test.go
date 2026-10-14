package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestDemuxManifestFrameReferencesPreserveExistingByteLimits(t *testing.T) {
	ctx := context.Background()
	s, err := vault.Initialize(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	a := evidence.OpenArchive(s, m)
	source, err := a.CommitCapture(ctx, evidence.CaptureDraft{Reader: strings.NewReader("container"), MaxBytes: 1024,
		MediaType: "video/x-msvideo", Complete: true, Provenance: evidence.Provenance{Kind: "asset-demux-source", Locator: "file:fixture.avi"}})
	if err != nil {
		t.Fatal(err)
	}
	frame, err := a.CommitCapture(ctx, evidence.CaptureDraft{Reader: strings.NewReader("frame"), MaxBytes: 1024,
		MediaType: "video/VP9", Complete: true, Provenance: evidence.Provenance{Kind: "asset-demux-frame", Locator: "fixture.avi#frame=0"}})
	if err != nil {
		t.Fatal(err)
	}
	// Repeated references keep this boundary fixture small on disk. Duplicating
	// these 15,000 IDs in metadata exceeds its existing 1 MiB cap, while the
	// typed demux payload remains comfortably inside its 16 MiB producer cap.
	manifest := AssetDemuxManifest{Schema: AssetDemuxSchema, Input: "fixture.avi", SourceCapture: source.ID,
		FrameCount: 15000, Frames: make([]AssetDemuxFrame, 15000), Complete: true}
	ids := []string{source.ID}
	for i := range manifest.Frames {
		manifest.Frames[i] = AssetDemuxFrame{Index: i, Capture: frame.ID, Blob: frame.Blob, SHA256: frame.Blob.SHA256, Size: 5}
		ids = append(ids, frame.ID)
	}
	wouldDuplicate, err := json.Marshal(ids)
	if err != nil || len(wouldDuplicate) <= 1<<20 {
		t.Fatal("fixture does not exercise the metadata document boundary")
	}
	raw, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil || len(raw) > 16<<20 {
		t.Fatal("fixture exceeds the existing demux manifest bound")
	}
	capture, err := commitDemuxManifest(ctx, s, m, manifest, raw, "")
	if err != nil {
		t.Fatalf("valid large demux manifest failed publication: %v", err)
	}
	for _, id := range []string{source.ID, frame.ID} {
		if _, err := a.RemoveCapture(ctx, id); !errors.Is(err, evidence.ErrCaptureReferenced) {
			t.Fatalf("large manifest lost dependency %s: %v", id, err)
		}
	}
	_, got, err := a.FetchCapture(ctx, capture.ID, 16<<20)
	if err != nil || !bytes.Equal(got, raw) {
		t.Fatalf("large manifest bytes changed: %v", err)
	}
	if _, err := a.RemoveCapture(ctx, capture.ID); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{source.ID, frame.ID} {
		if _, err := a.RemoveCapture(ctx, id); err != nil {
			t.Fatal(err)
		}
	}
	t.Logf("%d frames: %d manifest bytes, %d bytes if duplicated in metadata", len(manifest.Frames), len(raw), len(wouldDuplicate))
}
