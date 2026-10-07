package records

import (
	"context"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// fileSource is one authenticated CASC metadata source for a pinned build:
// either the explicitly named local installation or the explicitly selected
// CDN distribution. It owns its network/metadata handles and closes them with
// done; content extraction and evidence stay with the shared reader chain.
type fileSource struct {
	meta     BuildMetadata
	root     string // resolved installation root; empty for CDN sources
	open     func(context.Context, string, int64) (encodedObject, error)
	done     func()
	check    func(context.Context) error
	verify   func(context.Context) error
	setStats func(*DecodedCacheStats)
}

// prepareFileSource resolves the selected source against the immutable pin and
// rejects launcher metadata that disagrees with it. No legacy workspace,
// network fallback or locale fallback is consulted.
func prepareFileSource(ctx context.Context, store *vault.Store, pin selection.DataPin, q FileQuery) (fileSource, error) {
	if !validContentVariant(q.ContentVariant) {
		return fileSource{}, ErrFileQuery
	}
	product, _, err := selection.DataIdentity(pin)
	if err != nil {
		return fileSource{}, err
	}
	if store == nil || q.CDN && q.Installation != "" || !q.CDN && q.Installation == "" || q.FileDataID == 0 || q.MetadataBytes <= 0 || q.MetadataBytes > 1<<30 || q.ContentBytes <= 0 || q.ContentBytes > 1<<40 {
		return fileSource{}, ErrMetadataLimit
	}
	src := fileSource{done: func() {}}
	if q.CDN {
		remote, meta, err := prepareCDNFiles(ctx, store, pin, q.Offline, q.budget)
		if err != nil {
			return fileSource{}, err
		}
		src.meta, src.open = meta, remote.open
		remote.stats = q.cacheStats
		src.setStats = func(stats *DecodedCacheStats) { remote.stats = stats }
		src.done = func() { _ = remote.metadata.Close() }
	} else {
		installation, catalogProduct, err := dataInstallationRoot(ctx, q.Installation, product, pin.FullBuild)
		if err != nil {
			return fileSource{}, err
		}
		meta, err := ResolveLocalBuild(ctx, installation, catalogProduct, pin.FullBuild)
		if err != nil {
			return fileSource{}, err
		}
		if meta.Installed.BuildConfig != pin.BuildConfig || meta.Installed.CDNConfig != pin.CDNConfig {
			return fileSource{}, ErrPinnedBuildChanged
		}
		if meta.EncodingBytes > q.MetadataBytes || meta.ContentBytes > q.MetadataBytes {
			return fileSource{}, ErrMetadataLimit
		}
		local, err := openLocalSession(ctx, installation, q.budget)
		if err != nil {
			return fileSource{}, err
		}
		src.meta, src.root = meta, installation
		src.open, src.check, src.verify, src.done = local.open, local.check, local.verify, local.close
	}
	if src.meta.Installed.BuildConfig != pin.BuildConfig || src.meta.Installed.CDNConfig != pin.CDNConfig {
		src.done()
		return fileSource{}, ErrPinnedBuildChanged
	}
	if src.meta.EncodingBytes > q.MetadataBytes || src.meta.ContentBytes > q.MetadataBytes {
		src.done()
		return fileSource{}, ErrMetadataLimit
	}
	return src, nil
}

// encodingIndex authenticates and opens the CKey page directory of the
// source's Encoding file. The returned function releases the encoded object.
func (s fileSource) encodingIndex(ctx context.Context, q FileQuery) (*EncodingIndex, func(), error) {
	encoded, err := s.open(ctx, s.meta.EncodingKey, s.meta.EncodingBytes)
	if err != nil {
		return nil, nil, err
	}
	ranges, err := container.OpenRanges(ctx, encoded, s.meta.EncodingBytes, readLimits(s.meta.EncodingBytes, s.meta.ContentBytes, q.budget), q.Keys)
	if err != nil {
		_ = encoded.Close()
		return nil, nil, err
	}
	if ranges.Size() != s.meta.ContentBytes {
		_ = encoded.Close()
		return nil, nil, ErrMetadataFormat
	}
	index, err := openEncoding(ctx, ranges, q.budget)
	if err != nil {
		_ = encoded.Close()
		return nil, nil, err
	}
	index.enableSessionCache()
	return index, func() { _ = encoded.Close() }, nil
}
