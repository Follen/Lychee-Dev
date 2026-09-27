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
	keySource    *KeySource
	cacheStats   *DecodedCacheStats
	Installation string
	CDN          bool
	Offline      bool
	FileDataID   uint32
	// MetadataBytes bounds each Encoding/Root payload, ContentBytes the file.
	// Encoded and decoded sizes both count against their corresponding bound.
	MetadataBytes int64
	ContentBytes  int64
	Keys          container.KeyLookup
	KeyFile       string
}

type FileReading struct {
	DecodedCache *DecodedCacheStats `json:"decodedCache,omitempty"`
	KeySource    *KeySource         `json:"keySource,omitempty"`
	// PartialContent is separate from full CKey-verified Content.
	PartialContent     *vault.BlobRef          `json:"partialContent,omitempty"`
	Missing            []container.MissingSpan `json:"missing,omitempty"`
	ContentVerified    bool                    `json:"contentVerified"`
	Source             string                  `json:"source"`
	Pin                selection.DataPin       `json:"pin"`
	CatalogSHA256      string                  `json:"catalogSHA256"`
	BuildConfiguration vault.BlobRef           `json:"buildConfiguration"`
	CDNConfiguration   vault.BlobRef           `json:"cdnConfiguration"`
	EncodingKey        string                  `json:"encodingKey"`
	Root               vault.BlobRef           `json:"root"`
	Entry              RootRecord              `json:"entry"`
	PayloadEncodingKey string                  `json:"payloadEncodingKey"`
	Content            vault.BlobRef           `json:"content"`
}

// ReadFile verifies the selected source against the immutable pin, projects
// exactly one locale variant, and publishes only complete CKey-checked content.
// No legacy workspace, network fallback, locale fallback or global cache is used.
// Encoding is authenticated by EKey and visited page checks, not a claimed full
// Encoding CKey scan. Root and selected content are completely CKey checked.
func (r *Reader) ReadFile(ctx context.Context, pin selection.DataPin, q FileQuery) (FileReading, error) {
	return r.readFile(ctx, pin, q, false)
}

func (r *Reader) readFile(ctx context.Context, pin selection.DataPin, q FileQuery, allowMissing bool) (FileReading, error) {
	if err := ctx.Err(); err != nil {
		return FileReading{}, err
	}
	product, locale, err := selection.DataIdentity(pin)
	if err != nil {
		return FileReading{}, err
	}
	keys, keySource, err := r.prepareKeys(ctx, q)
	if err != nil {
		return FileReading{}, err
	}
	q.Keys = keys
	q.keySource = keySource
	q.cacheStats = &DecodedCacheStats{}
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
	var missing []container.MissingSpan
	var gaps *[]container.MissingSpan
	if allowMissing {
		gaps = &missing
	}
	content, key, err := r.extractContentWithCoverage(ctx, q, src.open, index, entry.ContentKey, q.ContentBytes, gaps)
	if err != nil {
		return FileReading{}, err
	}
	result := FileReading{Pin: pin, CatalogSHA256: meta.CatalogSHA256, EncodingKey: meta.EncodingKey, Root: root, Entry: entry, PayloadEncodingKey: key, Content: content}
	result.KeySource = keySource
	result.DecodedCache = q.cacheStats
	result.ContentVerified = len(missing) == 0
	if len(missing) > 0 {
		result.PartialContent, result.Missing, result.Content = &content, missing, vault.BlobRef{}
	}
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
	return r.extractContentWithCoverage(ctx, q, open, index, ckey, limit, nil)
}

func (r *Reader) extractContentWithCoverage(ctx context.Context, q FileQuery, open func(context.Context, string, int64) (encodedObject, error), index *EncodingIndex, ckey string, limit int64, missing *[]container.MissingSpan) (vault.BlobRef, string, error) {
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
		cacheKey, cacheErr := decodedCacheKey(ctx, q, object, physical.EncodedBytes, ckey)
		if cacheErr != nil {
			_ = object.Close()
			return vault.BlobRef{}, "", cacheErr
		}
		cached, hit, cacheErr := r.readDecodedCache(ctx, q, cacheKey, ckey, record.DecodedBytes)
		if cacheErr != nil {
			_ = object.Close()
			return vault.BlobRef{}, "", cacheErr
		}
		if hit {
			if err := object.Close(); err != nil {
				return vault.BlobRef{}, "", err
			}
			q.cacheStats.Hits++
			q.cacheStats.ReusedBytes += cached.Bytes
			return cached, key, nil
		}
		ref, extractErr := OpenPayloadArchive(r.store).ExtractContent(ctx, ContentInput{Encoded: object, ContentKey: ckey, Keys: q.Keys, Limits: readLimits(physical.EncodedBytes, max(1, record.DecodedBytes))})
		if extractErr == nil && cacheKey != "" {
			extractErr = r.saveDecodedCache(ctx, q, cacheKey, ckey, ref)
		}
		if missing != nil && errors.Is(extractErr, container.ErrKeyUnavailable) {
			ref, *missing, extractErr = r.extractAvailable(ctx, object, physical.EncodedBytes, record.DecodedBytes, q.Keys)
		}
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
