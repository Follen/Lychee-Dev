package evidence

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestCaptureDependenciesPublishAndReleaseWithOwner(t *testing.T) {
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
	a := OpenArchive(s, m)
	page, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("page bytes"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "page"}})
	if err != nil {
		t.Fatal(err)
	}
	dependencies := []string{page.ID}
	draft := CaptureDraft{Reader: strings.NewReader("owner bytes"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "owner"}, Dependencies: dependencies}
	owner, err := a.CommitCapture(ctx, draft)
	if err != nil {
		t.Fatal(err)
	}
	dependencies[0] = "CAP-changed-after-publication"
	if !reflect.DeepEqual(owner.Dependencies, []string{page.ID}) {
		t.Fatal("capture dependencies alias the caller's mutable slice")
	}
	observed, err := a.InspectCapture(ctx, owner.ID)
	if err != nil || !reflect.DeepEqual(observed, owner) {
		t.Fatalf("dependency manifest = %+v, %v", observed, err)
	}
	if _, err := a.KeepCapture(ctx, page.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := a.RemoveCapture(ctx, page.ID); !errors.Is(err, ErrCaptureReferenced) {
		t.Fatalf("owned page removal = %v", err)
	}
	if _, err := a.RemoveCapture(ctx, owner.ID); err != nil {
		t.Fatal(err)
	}
	if removed, err := a.RemoveCapture(ctx, page.ID); err != nil || !removed.ManifestRemoved || !removed.RetentionRemoved || !removed.BlobRemoved {
		t.Fatalf("released page cleanup = %+v, %v", removed, err)
	}
	draft.Reader, draft.Dependencies = strings.NewReader("unpublished bytes"), []string{page.ID}
	if _, err := a.CommitCapture(ctx, draft); !errors.Is(err, vault.ErrMissingRecord) {
		t.Fatalf("missing dependency publication = %v, want missing record", err)
	}
	if refs, err := a.ListCaptures(ctx, "", 10); err != nil || len(refs) != 0 {
		t.Fatalf("failed dependency publication left captures: %+v, %v", refs, err)
	}
}

func TestCaptureDependenciesArePartOfImmutableIdentity(t *testing.T) {
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
	a := OpenArchive(s, m)
	page, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("page"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "page"}})
	if err != nil {
		t.Fatal(err)
	}
	owner, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("owner"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "owner"}, Dependencies: []string{page.ID}})
	if err != nil {
		t.Fatal(err)
	}
	doc, err := m.ReadDocument(ctx, "capture/"+owner.ID)
	if err != nil {
		t.Fatal(err)
	}
	owner.Dependencies = nil
	raw, err := json.Marshal(owner)
	if err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, vault.Mutation{Key: doc.Key, ExpectedGeneration: doc.Generation, Value: raw}); err != nil {
		t.Fatal(err)
	}
	if _, err := a.InspectCapture(ctx, owner.ID); err == nil || err.Error() != "evidence.manifest_integrity" {
		t.Fatalf("changed dependencies accepted under original ID: %v", err)
	}
}

func TestCaptureDependencyPublicationExcludesRemoval(t *testing.T) {
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
	a := OpenArchive(s, m)
	page, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("page"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "page"}})
	if err != nil {
		t.Fatal(err)
	}
	validated, release := make(chan struct{}), make(chan struct{})
	capturedAt = func() time.Time { close(validated); <-release; return time.Now().UTC() }
	t.Cleanup(func() { capturedAt = func() time.Time { return time.Now().UTC() } })
	type result struct {
		ref CaptureRef
		err error
	}
	done := make(chan result, 1)
	go func() {
		ref, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("owner"), MaxBytes: 1024, MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "owner"}, Dependencies: []string{page.ID}})
		done <- result{ref, err}
	}()
	<-validated
	removeCtx, cancel := context.WithTimeout(ctx, 60*time.Millisecond)
	_, removeErr := a.RemoveCapture(removeCtx, page.ID)
	cancel()
	close(release)
	owner := <-done
	if owner.err != nil {
		t.Fatal(owner.err)
	}
	if !errors.Is(removeErr, context.DeadlineExceeded) {
		t.Fatalf("removal crossed dependency validation/publication: %v", removeErr)
	}
	if _, err := a.RemoveCapture(ctx, page.ID); !errors.Is(err, ErrCaptureReferenced) {
		t.Fatalf("committed dependency is not protected: %v", err)
	}
	if err := a.VerifyCapture(ctx, page.ID); err != nil {
		t.Fatal(err)
	}
}

func TestCaptureInspectPreservesPreDependencyManifestIdentity(t *testing.T) {
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
	blob, err := s.PublishBlob(ctx, vault.BlobInput{Reader: strings.NewReader("historical bytes"), MaxBytes: 1024})
	if err != nil {
		t.Fatal(err)
	}
	// Encode the exact old manifest shape independently of CaptureRef, so this
	// test detects changes to omitted fields or identity field ordering.
	old := struct {
		Schema     string        `json:"schema"`
		ID         string        `json:"id"`
		Blob       vault.BlobRef `json:"blob"`
		MediaType  string        `json:"mediaType"`
		Provenance Provenance    `json:"provenance"`
		Complete   bool          `json:"complete"`
		Truncated  bool          `json:"truncated"`
		CapturedAt time.Time     `json:"capturedAt"`
	}{Schema: "lycheedev.capture.v1", Blob: blob, MediaType: "text/plain", Provenance: Provenance{Kind: "fixture", Locator: "old"}, Complete: true, CapturedAt: time.Date(2026, 9, 23, 8, 0, 0, 0, time.UTC)}
	identity, err := json.Marshal(old)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(identity)
	old.ID = "CAP-" + hex.EncodeToString(digest[:])
	raw, err := json.Marshal(old)
	if err != nil {
		t.Fatal(err)
	}
	if err := m.CommitDocuments(ctx, vault.Mutation{Key: "capture/" + old.ID, Value: raw}); err != nil {
		t.Fatal(err)
	}
	ref, body, err := OpenArchive(s, m).FetchCapture(ctx, old.ID, 1024)
	if err != nil || ref.ID != old.ID || len(ref.Dependencies) != 0 || string(body) != "historical bytes" {
		t.Fatalf("historical manifest = %+v, %q, %v", ref, body, err)
	}
}
