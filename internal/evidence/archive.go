// Package evidence preserves original bytes and provenance independently of UI
// receipts, so interrupted live work never has to run again just to be inspected.
package evidence

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// capturedAt is the single source of capture timestamps. Tests pin it so that
// content-address identity collisions become deterministic instead of relying
// on scheduler timing.
var capturedAt = func() time.Time { return time.Now().UTC() }

type Provenance struct {
	Kind             string `json:"kind"`
	Locator          string `json:"locator"`
	Snapshot         string `json:"snapshot"`
	OperationID      string `json:"operationId"`
	SourceCommit     string `json:"sourceCommit,omitempty"`
	BaseSnapshot     string `json:"baseSnapshot,omitempty"`
	BaseSourceCommit string `json:"baseSourceCommit,omitempty"`
	DataBuild        string `json:"dataBuild,omitempty"`
	Session          string `json:"session,omitempty"`
}

type CaptureDraft struct {
	Reader     io.Reader
	MaxBytes   int64
	MediaType  string
	Provenance Provenance
	Complete   bool
	Truncated  bool
}

type CaptureRef struct {
	Schema     string        `json:"schema"`
	ID         string        `json:"id"`
	Blob       vault.BlobRef `json:"blob"`
	MediaType  string        `json:"mediaType"`
	Provenance Provenance    `json:"provenance"`
	Complete   bool          `json:"complete"`
	Truncated  bool          `json:"truncated"`
	CapturedAt time.Time     `json:"capturedAt"`
}

type Archive struct {
	store    *vault.Store
	metadata *vault.Metadata
}

func OpenArchive(store *vault.Store, metadata *vault.Metadata) *Archive {
	return &Archive{store: store, metadata: metadata}
}

func (a *Archive) CommitCapture(ctx context.Context, draft CaptureDraft) (CaptureRef, error) {
	if draft.Provenance.Kind == "" || draft.Provenance.Locator == "" || draft.MediaType == "" || draft.Complete && draft.Truncated {
		return CaptureRef{}, errors.New("evidence.invalid_capture")
	}
	blob, err := a.store.PublishBlob(ctx, vault.BlobInput{Reader: draft.Reader, MaxBytes: draft.MaxBytes})
	if err != nil {
		return CaptureRef{}, err
	}
	ref := CaptureRef{Schema: "lycheedev.capture.v1", Blob: blob, MediaType: draft.MediaType, Provenance: draft.Provenance, Complete: draft.Complete, Truncated: draft.Truncated, CapturedAt: capturedAt()}
	identity, err := json.Marshal(ref)
	if err != nil {
		return CaptureRef{}, err
	}
	digest := sha256.Sum256(identity)
	ref.ID = "CAP-" + hex.EncodeToString(digest[:])
	payload, err := json.Marshal(ref)
	if err != nil {
		return CaptureRef{}, err
	}
	if err := a.commitCaptureDocument(ctx, "capture/"+ref.ID, payload); err != nil {
		return CaptureRef{}, err
	}
	return ref, nil
}

// commitCaptureDocument inserts one immutable, content-addressed document. The
// key digests the whole manifest, so an insert generation conflict means
// another process committed an object under the same key. Immutable committed
// objects allow parallel access, and conflicting committers reuse the validated
// object instead of failing (design §9): an identically encoded stored document
// is accepted, while diverging bytes stay a conflict. Captures are never
// deleted, so the read-back after a conflict observes the winning document;
// the retry bound exists for transient read failures and never waits on
// wall-clock time.
func (a *Archive) commitCaptureDocument(ctx context.Context, key string, payload []byte) error {
	var err error
	for attempt := 0; attempt < 3; attempt++ {
		err = a.metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: payload})
		if err == nil {
			return nil
		}
		if !errors.Is(err, vault.ErrGeneration) {
			return err
		}
		if attempt == 2 {
			return err
		}
		doc, readErr := a.metadata.ReadDocument(ctx, key)
		if readErr != nil {
			continue
		}
		if bytes.Equal(doc.Value, payload) {
			return nil
		}
		return err
	}
	return err
}

func (a *Archive) InspectCapture(ctx context.Context, id string) (CaptureRef, error) {
	doc, err := a.metadata.ReadDocument(ctx, "capture/"+id)
	if err != nil {
		return CaptureRef{}, err
	}
	var ref CaptureRef
	if err := json.Unmarshal(doc.Value, &ref); err != nil {
		return ref, err
	}
	if ref.Schema != "lycheedev.capture.v1" || ref.ID != id || ref.Complete && ref.Truncated {
		return ref, errors.New("evidence.invalid_manifest")
	}
	unsigned := ref
	unsigned.ID = ""
	identity, err := json.Marshal(unsigned)
	if err != nil {
		return ref, err
	}
	digest := sha256.Sum256(identity)
	if ref.ID != "CAP-"+hex.EncodeToString(digest[:]) {
		return ref, errors.New("evidence.manifest_integrity")
	}
	return ref, nil
}

func (a *Archive) FetchCapture(ctx context.Context, id string, maxBytes int64) (CaptureRef, []byte, error) {
	ref, err := a.InspectCapture(ctx, id)
	if err != nil {
		return ref, nil, err
	}
	data, err := a.store.ReadBlob(ctx, ref.Blob, maxBytes)
	return ref, data, err
}

func (a *Archive) VerifyCapture(ctx context.Context, id string) error {
	ref, err := a.InspectCapture(ctx, id)
	if err != nil {
		return err
	}
	return a.store.VerifyBlob(ctx, ref.Blob)
}

func (a *Archive) ListCaptures(ctx context.Context, after string, limit int) ([]CaptureRef, error) {
	docs, err := a.metadata.ListDocuments(ctx, "capture/", "capture/"+after, limit)
	if err != nil {
		return nil, err
	}
	refs := make([]CaptureRef, 0, len(docs))
	for _, doc := range docs {
		var ref CaptureRef
		if err := json.Unmarshal(doc.Value, &ref); err != nil {
			return nil, err
		}
		checked, err := a.InspectCapture(ctx, ref.ID)
		if err != nil {
			return nil, err
		}
		refs = append(refs, checked)
	}
	return refs, nil
}
