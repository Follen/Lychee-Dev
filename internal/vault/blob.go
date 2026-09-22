package vault

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

var ErrBlobIntegrity = errors.New("vault.blob_integrity")
var ErrBlobLimit = errors.New("vault.blob_limit")

type BlobRef struct {
	SHA256 string `json:"sha256"`
	Bytes  int64  `json:"bytes"`
}

type BlobInput struct {
	Reader         io.Reader
	MaxBytes       int64
	ExpectedSHA256 string
}

// PublishBlob hashes original bytes while writing. Only complete, verified
// objects enter the content-addressed store; concurrent publishers coalesce.
func (s *Store) PublishBlob(ctx context.Context, input BlobInput) (BlobRef, error) {
	var zero BlobRef
	if input.Reader == nil || input.MaxBytes <= 0 || input.MaxBytes == 1<<63-1 {
		return zero, errors.New("vault: invalid blob input")
	}
	if input.ExpectedSHA256 != "" && !validDigest(input.ExpectedSHA256) {
		return zero, ErrBlobIntegrity
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	f, err := os.CreateTemp(filepath.Join(s.root, "tmp"), "blob-")
	if err != nil {
		return zero, err
	}
	defer os.Remove(f.Name())
	defer f.Close()
	hash := sha256.New()
	count, err := io.Copy(io.MultiWriter(f, hash), io.LimitReader(&interruptibleReader{ctx, input.Reader}, input.MaxBytes+1))
	if err != nil {
		return zero, err
	}
	if count > input.MaxBytes {
		return zero, ErrBlobLimit
	}
	ref := BlobRef{SHA256: hex.EncodeToString(hash.Sum(nil)), Bytes: count}
	if input.ExpectedSHA256 != "" && input.ExpectedSHA256 != ref.SHA256 {
		return zero, ErrBlobIntegrity
	}
	if err := f.Sync(); err != nil {
		return zero, err
	}
	if err := f.Close(); err != nil {
		return zero, err
	}
	lease, err := AcquireLease(ctx, filepath.Join(s.root, "locks"), "blob:"+ref.SHA256)
	if err != nil {
		return zero, err
	}
	defer lease.Close()
	destination := s.blobPath(ref.SHA256)
	if _, err := os.Lstat(destination); err == nil {
		if err := s.VerifyBlob(ctx, ref); err != nil {
			return zero, err
		}
		return ref, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return zero, err
	}
	if err := os.MkdirAll(filepath.Dir(destination), 0700); err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if err := os.Rename(f.Name(), destination); err != nil {
		return zero, err
	}
	return ref, nil
}

func (s *Store) VerifyBlob(ctx context.Context, ref BlobRef) error {
	if !validDigest(ref.SHA256) || ref.Bytes < 0 {
		return ErrBlobIntegrity
	}
	f, err := os.Open(s.blobPath(ref.SHA256))
	if err != nil {
		return err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return err
	}
	if !stat.Mode().IsRegular() || stat.Size() != ref.Bytes {
		return ErrBlobIntegrity
	}
	hash := sha256.New()
	count, err := io.Copy(hash, &interruptibleReader{ctx, f})
	if err != nil {
		return err
	}
	if count != ref.Bytes || hex.EncodeToString(hash.Sum(nil)) != ref.SHA256 {
		return ErrBlobIntegrity
	}
	return nil
}

// ReadBlob checks bounded content before exposing it to consumers.
func (s *Store) ReadBlob(ctx context.Context, ref BlobRef, maxBytes int64) ([]byte, error) {
	if !validDigest(ref.SHA256) || ref.Bytes < 0 {
		return nil, ErrBlobIntegrity
	}
	if maxBytes < 0 || ref.Bytes > maxBytes || ref.Bytes == 1<<63-1 {
		return nil, ErrBlobLimit
	}
	f, err := os.Open(s.blobPath(ref.SHA256))
	if err != nil {
		return nil, err
	}
	defer f.Close()
	data, err := io.ReadAll(io.LimitReader(&interruptibleReader{ctx, f}, ref.Bytes+1))
	if err != nil {
		return nil, err
	}
	digest := sha256.Sum256(data)
	if int64(len(data)) != ref.Bytes || hex.EncodeToString(digest[:]) != ref.SHA256 {
		return nil, ErrBlobIntegrity
	}
	return data, nil
}

func (s *Store) blobPath(digest string) string {
	return filepath.Join(s.root, "blobs", digest[:2], digest[2:])
}

func validDigest(value string) bool {
	if len(value) != 64 {
		return false
	}
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}

type interruptibleReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *interruptibleReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	n, err := r.source.Read(p)
	if n < 0 || n > len(p) {
		return 0, fmt.Errorf("vault: invalid reader count %d", n)
	}
	return n, err
}
