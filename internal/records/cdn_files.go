package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"path/filepath"
	"time"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type cdnFiles struct {
	ctx         context.Context
	store       *vault.Store
	metadata    *vault.Metadata
	client      *http.Client
	route       distributionRoute
	config      ConfigDocument
	offline     bool
	requests    int
	transferred int64
}

func prepareCDNFiles(ctx context.Context, store *vault.Store, pin selection.DataPin, offline bool) (*cdnFiles, BuildMetadata, error) {
	m, err := store.OpenMetadata(ctx)
	if err != nil {
		return nil, BuildMetadata{}, err
	}
	c := &cdnFiles{ctx: ctx, store: store, metadata: m, client: &http.Client{Timeout: 30 * time.Second}, offline: offline}
	meta, err := c.prepare(pin)
	if err != nil {
		m.Close()
		return nil, BuildMetadata{}, err
	}
	return c, meta, nil
}

func (c *cdnFiles) prepare(pin selection.DataPin) (BuildMetadata, error) {
	product, _, err := selection.DataIdentity(pin)
	if err != nil {
		return BuildMetadata{}, err
	}
	// A target's catalogs are delivery hints, never permission to reselect its
	// build. Explicit pins also work online without resolving latest versions.
	var distribution []byte
	doc, err := c.metadata.ReadDocument(c.ctx, "remote-catalog/"+product+"/"+pin.Region)
	if err == nil {
		var observed remoteObservation
		if json.Unmarshal(doc.Value, &observed) != nil || observed.ProductCode != product || observed.Region != pin.Region {
			return BuildMetadata{}, ErrRemoteIdentity
		}
		distribution, err = c.store.ReadBlob(c.ctx, observed.Distribution, 1<<20)
	} else if errors.Is(err, vault.ErrMissingRecord) {
		key := "cdn-route/" + product + "/" + pin.Region
		cached, cacheErr := c.metadata.ReadDocument(c.ctx, key)
		if cacheErr == nil {
			var ref vault.BlobRef
			if json.Unmarshal(cached.Value, &ref) != nil {
				return BuildMetadata{}, ErrRemoteIdentity
			}
			distribution, err = c.store.ReadBlob(c.ctx, ref, 1<<20)
		} else if !errors.Is(cacheErr, vault.ErrMissingRecord) {
			return BuildMetadata{}, cacheErr
		} else if c.offline {
			return BuildMetadata{}, ErrRemoteUnavailable
		} else {
			distribution, err = fetchRemoteMetadata(c.ctx, c.client, remoteVersionBase(pin.Region)+product+"/cdns", 1<<20)
			if err == nil {
				if _, err = parseDistributionCatalog(distribution, pin.Region); err != nil {
					return BuildMetadata{}, err
				}
				ref, publishErr := c.store.PublishBlob(c.ctx, vault.BlobInput{Reader: bytes.NewReader(distribution), MaxBytes: 1 << 20})
				if publishErr != nil {
					return BuildMetadata{}, publishErr
				}
				raw, _ := json.Marshal(ref)
				err = c.metadata.CommitDocuments(c.ctx, vault.Mutation{Key: key, Value: raw})
				if errors.Is(err, vault.ErrGeneration) {
					err = nil
				}
			}
		}
	}
	if err != nil {
		return BuildMetadata{}, err
	}
	c.route, err = parseDistributionCatalog(distribution, pin.Region)
	if err != nil {
		return BuildMetadata{}, err
	}
	buildDoc, _, err := loadRemoteConfiguration(c.ctx, c.store, c.metadata, c.client, c.route, pin.BuildConfig, c.offline)
	if err != nil {
		return BuildMetadata{}, err
	}
	c.config, _, err = loadRemoteConfiguration(c.ctx, c.store, c.metadata, c.client, c.route, pin.CDNConfig, c.offline)
	if err != nil {
		return BuildMetadata{}, err
	}
	return completeBuildMetadata(BuildMetadata{Installed: InstalledBuild{Product: product, FullBuild: pin.FullBuild, BuildConfig: pin.BuildConfig, CDNConfig: pin.CDNConfig}, BuildDocument: buildDoc, CDNDocument: c.config})
}

type cdnRange struct {
	ObjectBytes int64         `json:"objectBytes"`
	Blob        vault.BlobRef `json:"blob"`
}

// Cached fragments are original HTTP bytes, not proof of complete content.
// Parsers verify EKey/chunk/page identities and final decoded content CKeys.
func (c *cdnFiles) rangeBytes(object string, offset, length, total int64) ([]byte, int64, error) {
	if err := c.ctx.Err(); err != nil {
		return nil, 0, err
	}
	identity := fmt.Sprintf("%s/%s/%d/%d", c.route.Path, object, offset, length)
	sum := sha256.Sum256([]byte(identity))
	key := "cdn-range/" + hex.EncodeToString(sum[:])
	lease, err := vault.AcquireLease(c.ctx, filepath.Join(c.store.Root(), "locks"), key)
	if err != nil {
		return nil, 0, err
	}
	defer lease.Close()
	doc, err := c.metadata.ReadDocument(c.ctx, key)
	if err == nil {
		var saved cdnRange
		if json.Unmarshal(doc.Value, &saved) != nil || saved.ObjectBytes <= 0 || saved.ObjectBytes > 1<<50 || offset < 0 || offset > saved.ObjectBytes || length > saved.ObjectBytes-offset || saved.Blob.Bytes != length || total != 0 && saved.ObjectBytes != total {
			return nil, 0, ErrRemoteRange
		}
		raw, err := c.store.ReadBlob(c.ctx, saved.Blob, length)
		return raw, saved.ObjectBytes, err
	}
	if !errors.Is(err, vault.ErrMissingRecord) {
		return nil, 0, err
	}
	if c.offline {
		return nil, 0, ErrRemoteUnavailable
	}
	var last error = ErrRemoteObjectMissing
	for _, host := range c.route.Hosts {
		if c.requests >= 4096 || length > 1<<30-c.transferred {
			return nil, 0, ErrMetadataLimit
		}
		c.requests++
		c.transferred += length
		locator := "https://" + host + "/" + c.route.Path + "/data/" + object[:2] + "/" + object[2:4] + "/" + object
		raw, size, err := fetchRemoteRange(c.ctx, c.client, locator, offset, length, total)
		if err != nil {
			if c.ctx.Err() != nil {
				return nil, 0, c.ctx.Err()
			}
			if !errors.Is(err, ErrRemoteHTTP) && !errors.Is(err, ErrRemoteObjectMissing) {
				return nil, 0, fmt.Errorf("CDN object %s: %w", object, err)
			}
			// A failed mirror cannot turn an uncertain object into an absent one.
			if !errors.Is(err, ErrRemoteObjectMissing) || errors.Is(last, ErrRemoteObjectMissing) {
				last = err
			}
			continue
		}
		ref, err := c.store.PublishBlob(c.ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: length})
		if err != nil {
			return nil, 0, err
		}
		serialized, _ := json.Marshal(cdnRange{ObjectBytes: size, Blob: ref})
		if err := c.metadata.CommitDocuments(c.ctx, vault.Mutation{Key: key, Value: serialized}); err != nil {
			return nil, 0, err
		}
		return raw, size, nil
	}
	return nil, 0, last
}

type cdnBytes struct {
	files  *cdnFiles
	object string
	size   int64
	sparse bool
}

func (c *cdnFiles) source(object string, size int64, sparse bool) (*cdnBytes, error) {
	if size == 0 {
		_, total, err := c.rangeBytes(object, 0, 1, 0)
		if err != nil {
			return nil, err
		}
		size = total
	}
	return &cdnBytes{files: c, object: object, size: size, sparse: sparse}, nil
}

func (s *cdnBytes) ReadAt(p []byte, offset int64) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	if offset < 0 {
		return 0, ErrRemoteRange
	}
	if offset >= s.size {
		return 0, io.EOF
	}
	want := min(int64(len(p)), s.size-offset)
	if s.sparse {
		raw, _, err := s.files.rangeBytes(s.object, offset, want, s.size)
		if err != nil {
			return 0, err
		}
		n := copy(p, raw)
		if n < len(p) {
			return n, io.EOF
		}
		return n, nil
	}
	n := 0
	for int64(n) < want {
		const chunk int64 = 256 << 10
		at := offset + int64(n)
		base := at / chunk * chunk
		length := min(chunk, s.size-base)
		raw, _, err := s.files.rangeBytes(s.object, base, length, s.size)
		if err != nil {
			return n, err
		}
		copied := copy(p[n:int(want)], raw[at-base:])
		n += copied
	}
	if n < len(p) {
		return n, io.EOF
	}
	return n, nil
}

type remoteEncoded struct{ *io.SectionReader }

func (*remoteEncoded) Close() error { return nil }

func (c *cdnFiles) open(ctx context.Context, key string, size int64) (encodedObject, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !metadataKey(key) || size < 8 || size > 1<<40 {
		return nil, ErrMetadataFormat
	}
	locationKey := "cdn-location/" + c.config.Key + "/" + key
	var span cdnSpan
	doc, err := c.metadata.ReadDocument(ctx, locationKey)
	if err == nil {
		if json.Unmarshal(doc.Value, &span) != nil || span.Bytes != size || !metadataKey(span.Object) || span.Offset < 0 {
			return nil, ErrRemoteIdentity
		}
	} else if !errors.Is(err, vault.ErrMissingRecord) {
		return nil, err
	} else {
		span, err = c.locate(key, size)
		if err != nil {
			return nil, err
		}
	}
	// Total archive size is learned and cached separately from the object span.
	total := int64(0)
	if span.Object == key && span.Offset == 0 {
		total = size
	}
	source, err := c.source(span.Object, total, false)
	if err != nil {
		return nil, err
	}
	if span.Offset > source.size || span.Bytes > source.size-span.Offset {
		return nil, ErrRemoteRange
	}
	reader := io.NewSectionReader(source, span.Offset, span.Bytes)
	var prefix [8]byte
	if _, err := reader.ReadAt(prefix[:], 0); err != nil {
		return nil, err
	}
	if string(prefix[:4]) != "BLTE" {
		return nil, container.ErrMalformed
	}
	header := int64(binary.BigEndian.Uint32(prefix[4:]))
	if header != 0 && (header < 12 || header > size || header > 12+65536*24) {
		return nil, container.ErrMalformed
	}
	if header == 0 {
		header = size
	}
	hash := md5.New()
	if _, err := io.Copy(hash, io.NewSectionReader(reader, 0, header)); err != nil {
		return nil, err
	}
	if hex.EncodeToString(hash.Sum(nil)) != key {
		return nil, container.ErrIntegrity
	}
	if doc.Generation == 0 {
		raw, _ := json.Marshal(span)
		err = c.metadata.CommitDocuments(ctx, vault.Mutation{Key: locationKey, Value: raw})
		if err != nil && !errors.Is(err, vault.ErrGeneration) {
			return nil, err
		}
	}
	return &remoteEncoded{reader}, nil
}

func (c *cdnFiles) locate(key string, size int64) (cdnSpan, error) {
	// Encoding/Root often have loose copies even without file-index entries.
	_, _, err := c.rangeBytes(key, 0, 8, size)
	if err == nil {
		return cdnSpan{Object: key, Bytes: size}, nil
	}
	if !errors.Is(err, ErrRemoteObjectMissing) {
		return cdnSpan{}, err
	}
	archives := c.config.Fields["archives"]
	if len(archives) > 8192 {
		return cdnSpan{}, ErrMetadataLimit
	}
	for _, a := range archives {
		if !metadataKey(a) {
			return cdnSpan{}, ErrMetadataFormat
		}
	}
	lookup := func(indexKey, object string, expectedSize int64) (cdnSpan, error) {
		if !metadataKey(indexKey) {
			return cdnSpan{}, ErrMetadataFormat
		}
		source, err := c.source(indexKey+".index", expectedSize, true)
		if err != nil {
			return cdnSpan{}, err
		}
		index, err := openCDNIndex(c.ctx, source, source.size, indexKey)
		if err != nil {
			return cdnSpan{}, err
		}
		span, err := index.locate(c.ctx, key, archives, object)
		if err == nil && span.Bytes != size {
			return cdnSpan{}, ErrMetadataFormat
		}
		return span, err
	}
	if groups := c.config.Fields["archive-group"]; len(groups) > 0 {
		if len(groups) != 1 {
			return cdnSpan{}, ErrMetadataFormat
		}
		span, err := lookup(groups[0], "", 0)
		if err == nil {
			return span, nil
		}
		if !errors.Is(err, ErrRemoteObjectMissing) && !errors.Is(err, ErrContentMissing) {
			return cdnSpan{}, err
		}
	}
	// Published CN configurations have positional index-size hints that differ
	// from the actual keyed indexes. Discover length from a bounded response;
	// the footer identity and authenticated TOC establish the index's identity.
	for _, a := range archives {
		span, err := lookup(a, a, 0)
		if err == nil {
			return span, nil
		}
		if !errors.Is(err, ErrContentMissing) {
			return cdnSpan{}, err
		}
	}
	return cdnSpan{}, ErrRemoteObjectMissing
}
