package records

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrPinnedBuildChanged = errors.New("records.pinned_build_changed")
var ErrFileAmbiguous = errors.New("records.file_ambiguous")

type Reader struct {
	store         *vault.Store
	session       *readSession
	verifiedBlob  vault.BlobRef
	verifiedBytes []byte
	blobStats     *DecodedCacheStats
}

func OpenReader(store *vault.Store) *Reader { return &Reader{store: store} }

type FileQuery struct {
	budget          *resource.Budget
	admitted        bool
	binding         *readSession
	metadataContent bool
	keySource       *KeySource
	cacheStats      *DecodedCacheStats
	Installation    string
	CDN             bool
	Offline         bool
	FileDataID      uint32
	// Empty retains strict ambiguity detection. Explicit values filter only
	// the LOW_VIOLENCE bit; locale never implies a content-variant preference.
	ContentVariant string
	// MetadataBytes bounds each Encoding/Root payload, ContentBytes the file.
	// Encoded and decoded sizes both count against their corresponding bound.
	MetadataBytes int64
	ContentBytes  int64
	Keys          container.KeyLookup
	KeyFile       string
}

type FileReading struct {
	ContentVariant string             `json:"contentVariant,omitempty"`
	DecodedCache   *DecodedCacheStats `json:"decodedCache,omitempty"`
	KeySource      *KeySource         `json:"keySource,omitempty"`
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
	if !validContentVariant(q.ContentVariant) {
		return FileReading{}, ErrFileQuery
	}
	if r == nil || r.store == nil || q.CDN && q.Installation != "" || !q.CDN && q.Installation == "" || q.FileDataID == 0 || q.MetadataBytes <= 0 || q.MetadataBytes > 1<<30 || q.ContentBytes <= 0 || q.ContentBytes > 1<<40 {
		return FileReading{}, ErrMetadataLimit
	}
	q = ensureQueryBudget(q)
	if !q.admitted {
		lease, err := r.store.AcquireDataResources(ctx, vault.DataOrdinary)
		if err != nil {
			return FileReading{}, err
		}
		defer lease.Close()
		q.admitted = true
	}
	if err := ctx.Err(); err != nil {
		return FileReading{}, err
	}
	if !validContentVariant(q.ContentVariant) {
		return FileReading{}, ErrFileQuery
	}
	product, locale, err := selection.DataIdentity(pin)
	if err != nil {
		return FileReading{}, err
	}
	session := r.session
	if session == nil {
		owned, err := r.NewSession(ctx, pin, q)
		if err != nil {
			return FileReading{}, err
		}
		defer owned.Close()
		result, err := owned.readFile(ctx, pin, owned.Query(), allowMissing)
		if err == nil {
			err = owned.CheckSource(ctx)
		}
		return result, err
	}
	if err := session.prepare(ctx, r, pin, q); err != nil {
		return FileReading{}, err
	}
	q.Keys, q.keySource, q.budget = session.keys, session.keySource, session.query.budget
	q.cacheStats = &DecodedCacheStats{}
	if !session.rootReported {
		*q.cacheStats = session.rootStats
	}
	if session.source.setStats != nil {
		session.source.setStats(q.cacheStats)
	}
	src, index, root := session.source, session.index, session.root
	meta := src.meta
	entries, err := session.rootIndex.lookup(ctx, q.FileDataID)
	if err != nil {
		return FileReading{}, err
	}
	entry, err := chooseFile(entries, q.FileDataID, locale, q.ContentVariant)
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
	result.KeySource = session.keySource
	result.ContentVariant = q.ContentVariant
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
	if err := session.check(ctx); err != nil {
		return FileReading{}, err
	}
	session.rootReported = true
	return result, nil
}

func validContentVariant(variant string) bool {
	return variant == "" || variant == "standard" || variant == "low-violence"
}

func chooseFile(entries []RootRecord, id, locale uint32, variant string) (RootRecord, error) {
	if !validContentVariant(variant) {
		return RootRecord{}, ErrFileQuery
	}
	var selected RootRecord
	found := false
	for _, entry := range entries {
		if entry.FileDataID != id || entry.LocaleMask&locale == 0 {
			continue
		}
		// CASC_CFLAG_LOW_VIOLENCE=0x80. This is an explicit bit filter, not
		// emulation of a running client's overrideArchive or other preferences.
		lowViolence := entry.ContentFlags&0x80 != 0
		if variant == "standard" && lowViolence || variant == "low-violence" && !lowViolence {
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
	if q.metadataContent {
		if err := q.budget.Charge(resource.Cost{MetadataBytes: record.DecodedBytes}); err != nil {
			return vault.BlobRef{}, "", err
		}
	}
	var unavailable error
	for _, key := range record.EncodingKeys {
		if err := ctx.Err(); err != nil {
			return vault.BlobRef{}, "", err
		}
		physical, err := index.FindEncoding(ctx, key)
		if err != nil {
			return vault.BlobRef{}, "", err
		}
		if physical.EncodedBytes > limit {
			return vault.BlobRef{}, "", ErrMetadataLimit
		}
		ref, hit, gaps, err := r.tryEncoding(ctx, q, open, key, ckey, physical.EncodedBytes, record.DecodedBytes, missing != nil)
		if !errors.Is(err, resource.ErrBudget) && !errors.Is(err, ErrMetadataLimit) && !errors.Is(err, errEncodingClose) && !errors.Is(err, context.Canceled) && !errors.Is(err, context.DeadlineExceeded) && (errors.Is(err, ErrObjectUnavailable) || remoteAvailabilityFailure(err)) {
			if unavailable == nil || errors.Is(err, ErrRemoteHTTP) {
				unavailable = err
			}
			continue
		}
		if err != nil {
			return vault.BlobRef{}, "", err
		}
		if hit {
			q.cacheStats.Hits++
			q.cacheStats.ReusedBytes += ref.Bytes
		}
		if missing != nil {
			*missing = gaps
		}
		return ref, key, nil
	}
	if unavailable == nil {
		unavailable = ErrObjectUnavailable
	}
	return vault.BlobRef{}, "", fmt.Errorf("%w: no available encoding for content %s", unavailable, ckey)
}

var errEncodingClose = errors.New("records.encoding_close")

// Availability covers the entire lazy object read, not just opening it. State
// belongs to one candidate until its extraction and close both succeed.
func (r *Reader) tryEncoding(ctx context.Context, q FileQuery, open func(context.Context, string, int64) (encodedObject, error), key, ckey string, encoded, decoded int64, allowMissing bool) (ref vault.BlobRef, hit bool, missing []container.MissingSpan, err error) {
	r.blobStats = q.cacheStats
	if err = q.budget.Charge(resource.Cost{DecodeWork: (encoded + decoded + 65535) / 65536}); err != nil {
		return ref, false, nil, err
	}
	object, err := open(ctx, key, encoded)
	if err != nil {
		return ref, false, nil, err
	}
	defer func() {
		if closeErr := object.Close(); closeErr != nil {
			err = errors.Join(err, fmt.Errorf("%w: %w", errEncodingClose, closeErr))
		}
	}()
	cacheKey, err := decodedCacheKey(ctx, q, object, encoded, ckey)
	if err != nil {
		return ref, false, nil, err
	}
	ref, hit, err = r.readDecodedCache(ctx, q, cacheKey, ckey, decoded)
	if err != nil || hit {
		return ref, hit, nil, err
	}
	if q.cacheStats != nil {
		q.cacheStats.ContentDecodeCalls++
	}
	ref, err = OpenPayloadArchive(r.store).ExtractContent(ctx, ContentInput{Encoded: object, ContentKey: ckey, Keys: q.Keys, Limits: readLimits(encoded, max(1, decoded), q.budget)})
	if allowMissing && errors.Is(err, container.ErrKeyUnavailable) {
		if q.cacheStats != nil {
			q.cacheStats.ContentDecodeCalls++
		}
		ref, missing, err = r.extractAvailable(ctx, object, encoded, decoded, q.Keys, q.budget)
	}
	if err != nil {
		return ref, false, nil, err
	}
	if ref.Bytes != decoded {
		return ref, false, nil, ErrMetadataFormat
	}
	if q.cacheStats != nil {
		q.cacheStats.ExtractedBytes += ref.Bytes
	}
	if cacheKey != "" && len(missing) == 0 {
		err = r.saveDecodedCache(ctx, q, cacheKey, ckey, ref)
	}
	return ref, false, missing, err
}

func readLimits(encoded, decoded int64, budgets ...*resource.Budget) container.Limits {
	limits := container.Limits{EncodedBytes: encoded, DecodedBytes: decoded, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16}
	if len(budgets) > 0 {
		limits.Query = budgets[0]
	}
	return limits
}
