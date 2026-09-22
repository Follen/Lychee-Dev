package records

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrPinnedBuildChanged = errors.New("records.pinned_build_changed")
var ErrFileAmbiguous = errors.New("records.file_ambiguous")

type Reader struct{ store *vault.Store }

func OpenReader(store *vault.Store) *Reader { return &Reader{store: store} }

type FileQuery struct {
	Installation string
	CDN          bool
	Offline      bool
	FileDataID   uint32
	// MetadataBytes bounds each Encoding/Root payload, ContentBytes the file.
	// Encoded and decoded sizes both count against their corresponding bound.
	MetadataBytes int64
	ContentBytes  int64
	Keys          container.KeyLookup
}

type FileReading struct {
	Source             string            `json:"source"`
	Pin                selection.DataPin `json:"pin"`
	CatalogSHA256      string            `json:"catalogSHA256"`
	BuildConfiguration vault.BlobRef     `json:"buildConfiguration"`
	CDNConfiguration   vault.BlobRef     `json:"cdnConfiguration"`
	EncodingKey        string            `json:"encodingKey"`
	Root               vault.BlobRef     `json:"root"`
	Entry              RootRecord        `json:"entry"`
	PayloadEncodingKey string            `json:"payloadEncodingKey"`
	Content            vault.BlobRef     `json:"content"`
}

// ReadFile verifies the selected source against the immutable pin, projects
// exactly one locale variant, and publishes only complete CKey-checked content.
// No legacy workspace, network fallback, locale fallback or global cache is used.
// Encoding is authenticated by EKey and visited page checks, not a claimed full
// Encoding CKey scan. Root and selected content are completely CKey checked.
func (r *Reader) ReadFile(ctx context.Context, pin selection.DataPin, q FileQuery) (FileReading, error) {
	if err := ctx.Err(); err != nil {
		return FileReading{}, err
	}
	product, locale, err := selection.DataIdentity(pin)
	if err != nil {
		return FileReading{}, err
	}
	src, err := prepareFileSource(ctx, r.store, pin, q)
	if err != nil {
		return FileReading{}, err
	}
	defer src.done()
	meta := src.meta
	index, closeIndex, err := src.encodingIndex(ctx, q)
	if err != nil {
		return FileReading{}, err
	}
	defer closeIndex()
	root, _, err := r.extractContent(ctx, q, src.open, index, meta.RootContentKey, q.MetadataBytes)
	if err != nil {
		return FileReading{}, fmt.Errorf("root: %w", err)
	}
	raw, err := r.store.ReadBlob(ctx, root, q.MetadataBytes)
	if err != nil {
		return FileReading{}, err
	}
	entries, err := LookupRoot(ctx, bytes.NewReader(raw), int64(len(raw)), []uint32{q.FileDataID}, RootLimits{Bytes: q.MetadataBytes, Records: 10000000, Groups: 65536, Matches: 4096})
	if err != nil {
		return FileReading{}, err
	}
	entry, err := chooseFile(entries, q.FileDataID, locale)
	if err != nil {
		return FileReading{}, err
	}
	content, key, err := r.extractContent(ctx, q, src.open, index, entry.ContentKey, q.ContentBytes)
	if err != nil {
		return FileReading{}, err
	}
	result := FileReading{Pin: pin, CatalogSHA256: meta.CatalogSHA256, EncodingKey: meta.EncodingKey, Root: root, Entry: entry, PayloadEncodingKey: key, Content: content}
	result.Source = "installation"
	if q.CDN {
		result.Source = "cdn"
	}
	result.BuildConfiguration, err = r.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(meta.BuildDocument.Raw), MaxBytes: 4 << 20, ExpectedSHA256: meta.BuildDocument.SHA256})
	if err != nil {
		return FileReading{}, err
	}
	result.CDNConfiguration, err = r.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(meta.CDNDocument.Raw), MaxBytes: 4 << 20, ExpectedSHA256: meta.CDNDocument.SHA256})
	if err != nil {
		return FileReading{}, err
	}
	// Detect launcher metadata changes during the read. Already published immutable
	// blobs may be reused, but no successful reading may claim a different pin.
	if !q.CDN {
		after, err := ResolveLocalBuild(ctx, src.root, product, pin.FullBuild)
		if err != nil {
			return FileReading{}, err
		}
		if after.Installed != meta.Installed {
			return FileReading{}, ErrPinnedBuildChanged
		}
	}
	return result, nil
}

func chooseFile(entries []RootRecord, id, locale uint32) (RootRecord, error) {
	var selected RootRecord
	found := false
	for _, entry := range entries {
		if entry.FileDataID != id || entry.LocaleMask&locale == 0 {
			continue
		}
		// Content variants are never silently ranked, even if their CKeys match.
		if found {
			return RootRecord{}, ErrFileAmbiguous
		}
		selected, found = entry, true
	}
	if !found {
		return RootRecord{}, ErrContentMissing
	}
	return selected, nil
}

type encodedObject interface {
	io.Reader
	io.ReaderAt
	io.Closer
}

func (r *Reader) extractContent(ctx context.Context, q FileQuery, open func(context.Context, string, int64) (encodedObject, error), index *EncodingIndex, ckey string, limit int64) (vault.BlobRef, string, error) {
	record, err := index.FindContent(ctx, ckey)
	if err != nil {
		return vault.BlobRef{}, "", err
	}
	if record.DecodedBytes > limit {
		return vault.BlobRef{}, "", ErrMetadataLimit
	}
	for _, key := range record.EncodingKeys {
		physical, err := index.FindEncoding(ctx, key)
		if err != nil {
			return vault.BlobRef{}, "", err
		}
		if physical.EncodedBytes > limit {
			return vault.BlobRef{}, "", ErrMetadataLimit
		}
		object, err := open(ctx, key, physical.EncodedBytes)
		if errors.Is(err, ErrObjectUnavailable) || errors.Is(err, ErrRemoteObjectMissing) {
			continue
		}
		if err != nil {
			return vault.BlobRef{}, "", err
		}
		// Decoder/store budgets are positive; an empty file still has a BLTE
		// representation. The CKey and exact decoded length below remain binding.
		ref, extractErr := OpenPayloadArchive(r.store).ExtractContent(ctx, ContentInput{Encoded: object, ContentKey: ckey, Keys: q.Keys, Limits: readLimits(physical.EncodedBytes, max(1, record.DecodedBytes))})
		closeErr := object.Close()
		if extractErr != nil {
			return vault.BlobRef{}, "", extractErr
		}
		if closeErr != nil {
			return vault.BlobRef{}, "", closeErr
		}
		if ref.Bytes != record.DecodedBytes {
			return vault.BlobRef{}, "", ErrMetadataFormat
		}
		return ref, key, nil
	}
	return vault.BlobRef{}, "", ErrObjectUnavailable
}

func readLimits(encoded, decoded int64) container.Limits {
	return container.Limits{EncodedBytes: encoded, DecodedBytes: decoded, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16}
}
