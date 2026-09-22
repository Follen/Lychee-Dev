package records

import (
	"context"
	"crypto/md5"
	"encoding/hex"
	"errors"
	"io"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/vault"
)

// PayloadArchive publishes decoded CASC content into the shared object store.
// It has no global keys, old-tool cache, network client or workspace discovery.
type PayloadArchive struct{ store *vault.Store }

func OpenPayloadArchive(store *vault.Store) *PayloadArchive { return &PayloadArchive{store: store} }

type ContentInput struct {
	Encoded io.Reader
	// ContentKey is the full lowercase MD5 of decoded content, not the encoding
	// key. MD5 is a CASC format identity; vault uses SHA-256 for internal storage.
	ContentKey string
	Limits     container.Limits
	Keys       container.KeyLookup
}

// ExtractContent streams through a bounded decoder into a staged vault object.
// Nothing is published unless decoding and the external content key both verify.
// Pin-to-content provenance is owned by the higher-level data resolver.
func (a *PayloadArchive) ExtractContent(ctx context.Context, input ContentInput) (vault.BlobRef, error) {
	key, err := hex.DecodeString(input.ContentKey)
	if err != nil || len(key) != md5.Size || hex.EncodeToString(key) != input.ContentKey {
		return vault.BlobRef{}, errors.New("records.invalid_content_key")
	}
	if a.store == nil || input.Encoded == nil {
		return vault.BlobRef{}, errors.New("records.invalid_content_input")
	}
	reader, writer := io.Pipe()
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		hash := md5.New()
		_, decodeErr := container.DecodeWithKeys(ctx, io.MultiWriter(writer, hash), input.Encoded, input.Limits, input.Keys)
		if decodeErr == nil && hex.EncodeToString(hash.Sum(nil)) != input.ContentKey {
			decodeErr = container.ErrIntegrity
		}
		_ = writer.CloseWithError(decodeErr)
	}()
	ref, publishErr := a.store.PublishBlob(ctx, vault.BlobInput{Reader: reader, MaxBytes: input.Limits.DecodedBytes})
	// Always unblock a decoder writing to a failed/cancelled publication before
	// joining it. The caller's source must itself support bounded/cancellable I/O.
	_ = reader.CloseWithError(publishErr)
	<-finished
	return ref, publishErr
}
