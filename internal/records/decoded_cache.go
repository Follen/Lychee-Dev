package records

import (
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"os"
)

type DecodedCacheStats struct {
	Hits        int   `json:"hits"`
	ReusedBytes int64 `json:"reusedBytes"`
}
type decodedCacheEntry struct {
	ContentKey string        `json:"contentKey"`
	Blob       vault.BlobRef `json:"blob"`
	Keys       KeySource     `json:"keys"`
}

// Reuse only fully verified decoded content. The selected encoded source is
// still opened and hashed on every call, so reuse cannot mask changed/corrupt
// input or substitute another installation/CDN. Callback key providers have no
// stable identity and therefore never participate. Partial bytes never enter.
func decodedCacheKey(ctx context.Context, q FileQuery, source io.ReaderAt, size int64, ckey string) (string, error) {
	if q.keySource == nil || q.keySource.Kind == "provided" {
		return "", nil
	}
	hash := sha256.New()
	fmt.Fprintf(hash, "decoded-v1\x00%s\x00%s\x00%s\x00", ckey, q.keySource.Kind, q.keySource.SHA256)
	buffer := make([]byte, 64<<10)
	for offset := int64(0); offset < size; {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		n := min(int64(len(buffer)), size-offset)
		read, err := source.ReadAt(buffer[:n], offset)
		if int64(read) != n {
			return "", io.ErrUnexpectedEOF
		}
		if err != nil && err != io.EOF {
			return "", err
		}
		hash.Write(buffer[:read])
		offset += int64(read)
	}
	return "decoded-v1/" + hex.EncodeToString(hash.Sum(nil)), nil
}

func (r *Reader) readDecodedCache(ctx context.Context, q FileQuery, key, ckey string, size int64) (vault.BlobRef, bool, error) {
	if key == "" {
		return vault.BlobRef{}, false, nil
	}
	metadata, err := r.store.OpenMetadata(ctx)
	if err != nil {
		return vault.BlobRef{}, false, err
	}
	defer metadata.Close()
	doc, err := metadata.ReadDocument(ctx, key)
	if errors.Is(err, vault.ErrMissingRecord) {
		return vault.BlobRef{}, false, nil
	}
	if err != nil {
		return vault.BlobRef{}, false, err
	}
	var cached decodedCacheEntry
	if json.Unmarshal(doc.Value, &cached) != nil || cached.ContentKey != ckey || cached.Blob.Bytes != size || cached.Keys.Kind != q.keySource.Kind || cached.Keys.SHA256 != q.keySource.SHA256 {
		return vault.BlobRef{}, false, vault.ErrBlobIntegrity
	}
	if cached.Keys.Kind == "public" && cached.Keys.State == "loaded" {
		// Re-establish current key-document availability, including offline behavior.
		_, lookupErr := q.Keys(ctx, 0)
		if q.keySource.State != "loaded" {
			if errors.Is(lookupErr, container.ErrKeyUnavailable) {
				return vault.BlobRef{}, false, nil
			}
			return vault.BlobRef{}, false, lookupErr
		}
	}
	raw, err := r.store.ReadBlob(ctx, cached.Blob, max(1, size))
	if errors.Is(err, os.ErrNotExist) {
		return vault.BlobRef{}, false, nil
	}
	if err != nil {
		return vault.BlobRef{}, false, err
	}
	digest := md5.Sum(raw)
	if hex.EncodeToString(digest[:]) != ckey {
		return vault.BlobRef{}, false, vault.ErrBlobIntegrity
	}
	return cached.Blob, true, nil
}

func (r *Reader) saveDecodedCache(ctx context.Context, q FileQuery, key, ckey string, blob vault.BlobRef) error {
	metadata, err := r.store.OpenMetadata(ctx)
	if err != nil {
		return err
	}
	defer metadata.Close()
	raw, err := json.Marshal(decodedCacheEntry{ContentKey: ckey, Blob: blob, Keys: *q.keySource})
	if err != nil {
		return err
	}
	// Concurrent exact-content readers can race to publish the same identity.
	if _, err := metadata.ReadDocument(ctx, key); err == nil {
		return nil
	} else if !errors.Is(err, vault.ErrMissingRecord) {
		return err
	}
	err = metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: raw})
	if errors.Is(err, vault.ErrGeneration) {
		_, _, readErr := r.readDecodedCache(ctx, q, key, ckey, blob.Bytes)
		return readErr
	}
	return err
}
