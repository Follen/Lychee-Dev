package records

import (
	"bytes"
	"context"
	"fmt"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// A session is owned by one query, never shared across CLI requests. Preparation
// is lazy so definition-only/empty queries do not open CASC. The caller checks
// the source again before publishing the query capture and always closes it.
type readSession struct {
	pin           selection.DataPin
	query         FileQuery
	keys          container.KeyLookup
	keySource     *KeySource
	source        fileSource
	index         *EncodingIndex
	closeIndex    func()
	root          vault.BlobRef
	rootBytes     []byte
	rootStats     DecodedCacheStats
	rootReported  bool
	rootIndex     *rootIndex
	ready, closed bool
	views         map[string]preparedSessionTable
	hotfix        *hotfixSnapshots
	admission     *vault.DataResources
}

func (r *Reader) NewSession(ctx context.Context, pin selection.DataPin, q FileQuery) (*Reader, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if r == nil || r.store == nil || r.session != nil {
		return nil, ErrFileQuery
	}
	if _, _, err := selection.DataIdentity(pin); err != nil {
		return nil, err
	}
	q = ensureQueryBudget(q)
	var admission *vault.DataResources
	if !q.admitted {
		var err error
		admission, err = r.store.AcquireDataResources(ctx, vault.DataOrdinary)
		if err != nil {
			return nil, err
		}
		q.admitted = true
	}
	return &Reader{store: r.store, session: &readSession{pin: pin, query: q, views: make(map[string]preparedSessionTable), admission: admission}}, nil
}

// Query returns the fixed source options plus an unforgeable in-process binding.
// The key provider belongs to the session; function code pointers are not
// identities for closures and are never used to authorize another provider.
func (r *Reader) Query() FileQuery {
	if r.session == nil {
		return FileQuery{}
	}
	query := r.session.query
	query.Keys = nil
	query.binding = r.session
	return query
}

func (s *readSession) validate(pin selection.DataPin, q FileQuery) error {
	if s.closed || pin != s.pin || q.Installation != s.query.Installation || q.CDN != s.query.CDN || q.Offline != s.query.Offline || q.KeyFile != s.query.KeyFile || q.ContentVariant != s.query.ContentVariant || q.MetadataBytes != s.query.MetadataBytes || q.ContentBytes > s.query.ContentBytes || q.Keys != nil || s.query.Keys != nil && q.binding != s || q.binding != nil && q.binding != s {
		return ErrFileQuery
	}
	return nil
}

func (s *readSession) prepare(ctx context.Context, r *Reader, pin selection.DataPin, q FileQuery) error {
	if err := s.validate(pin, q); err != nil {
		return err
	}
	if s.ready {
		return s.check(ctx)
	}
	keys, info, err := r.prepareKeys(ctx, s.query)
	if err != nil {
		return err
	}
	q.Keys, q.keySource, q.budget, q.cacheStats = keys, info, s.query.budget, &DecodedCacheStats{}
	q.metadataContent = true
	source, err := prepareFileSource(ctx, r.store, pin, q)
	if err != nil {
		return err
	}
	index, closeIndex, err := source.encodingIndex(ctx, q)
	if err != nil {
		source.done()
		return err
	}
	keep := false
	defer func() {
		if !keep {
			closeIndex()
			source.done()
		}
	}()
	root, _, err := r.extractContent(ctx, q, source.open, index, source.meta.RootContentKey, q.MetadataBytes)
	if err != nil {
		return fmt.Errorf("root: %w", err)
	}
	raw, err := r.readVerifiedBlob(ctx, root, q.MetadataBytes)
	if err != nil {
		return err
	}
	rootDirectory, err := newRootIndex(ctx, bytes.NewReader(raw), int64(len(raw)), RootLimits{Query: q.budget, Bytes: q.MetadataBytes, Records: 10000000, Groups: 65536, Matches: 4096})
	if err != nil {
		return err
	}
	s.rootIndex = rootDirectory
	s.keys, s.keySource, s.source, s.index, s.closeIndex, s.root, s.rootBytes = keys, info, source, index, closeIndex, root, raw
	s.rootStats = *q.cacheStats
	s.ready, keep = true, true
	return nil
}

func (s *readSession) check(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if s.closed {
		return ErrFileQuery
	}
	if !s.ready || s.query.CDN {
		return nil
	}
	product, _, err := selection.DataIdentity(s.pin)
	if err != nil {
		return err
	}
	after, err := ResolveLocalBuild(ctx, s.source.root, product, s.pin.FullBuild)
	if err != nil {
		return err
	}
	if after.Installed != s.source.meta.Installed {
		return ErrPinnedBuildChanged
	}
	if s.source.check != nil {
		return s.source.check(ctx)
	}
	return nil
}

func (r *Reader) CheckSource(ctx context.Context) error {
	if r.session == nil {
		return ctx.Err()
	}
	if err := r.session.check(ctx); err != nil {
		return err
	}
	if r.session.ready && r.session.source.verify != nil {
		return r.session.source.verify(ctx)
	}
	return nil
}

func (r *Reader) Close() error {
	if r.session == nil || r.session.closed {
		return nil
	}
	s := r.session
	s.closed = true
	if s.closeIndex != nil {
		s.closeIndex()
	}
	if s.source.done != nil {
		s.source.done()
	}
	s.rootBytes = nil
	s.rootIndex = nil
	s.views = nil
	s.hotfix = nil
	s.query = FileQuery{}
	s.keys, s.keySource = nil, nil
	s.source, s.index, s.closeIndex = fileSource{}, nil, nil
	s.root, s.rootStats = vault.BlobRef{}, DecodedCacheStats{}
	r.verifiedBlob, r.blobStats = vault.BlobRef{}, nil
	r.verifiedBytes = nil
	if s.admission != nil {
		admission := s.admission
		s.admission = nil
		return admission.Close()
	}
	return nil
}

// Reuse only the most recently validated immutable bytes. Every new request
// rehashes vault blobs; a returned cache hit and its immediate consumer share
// the same authenticated bytes rather than rereading a mutable path.
func (r *Reader) readVerifiedBlob(ctx context.Context, ref vault.BlobRef, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if ref.Bytes > limit {
		return nil, vault.ErrBlobIntegrity
	}
	if r.verifiedBytes != nil && r.verifiedBlob == ref {
		return r.verifiedBytes, nil
	}
	if r.session != nil {
		if err := r.session.query.budget.Charge(resource.Cost{RetainedBytes: ref.Bytes}); err != nil {
			return nil, err
		}
	}
	raw, err := r.store.ReadBlob(ctx, ref, limit)
	if err != nil {
		return nil, err
	}
	if r.blobStats != nil {
		r.blobStats.BlobReads++
		r.blobStats.BlobReadBytes += int64(len(raw))
	}
	r.verifiedBlob, r.verifiedBytes = ref, raw
	return raw, nil
}
