// SPDX-License-Identifier: AGPL-3.0-or-later
// Listfile source handling adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// Default public listfile locations retained from the legacy toolkit. They are
// only defaults of an explicit source spec; nothing downloads until a caller
// selects a kind and allows network access.
var defaultListfileURLs = map[ListfileKind][]string{
	ListfileCommunityCSV:    {"https://github.com/wowdev/wow-listfile/releases/latest/download/community-listfile.csv"},
	ListfileWowExportText:   {"https://www.kruithne.net/wow.export/data/listfile/master"},
	ListfileWowExportBinary: {"https://www.kruithne.net/wow.export/data/listfile/bin/%s"},
}

// ListfileSourceSpec returns the known URL manifest for one listfile kind.
// Binary URLs are component templates consumed with fmt.Sprintf semantics.
func ListfileSourceSpec(kind ListfileKind) []string {
	urls := defaultListfileURLs[kind]
	return append([]string(nil), urls...)
}

// ListfileFetcher downloads one listfile payload with a hard byte budget. The
// transport is a real external dependency and stays outside the module; the
// offline mode never calls it.
type ListfileFetcher interface {
	FetchListfile(ctx context.Context, url string, maxBytes int64) ([]byte, error)
}

// ListfileFetchFunc adapts a function to ListfileFetcher.
type ListfileFetchFunc func(ctx context.Context, url string, maxBytes int64) ([]byte, error)

func (f ListfileFetchFunc) FetchListfile(ctx context.Context, url string, maxBytes int64) ([]byte, error) {
	return f(ctx, url, maxBytes)
}

// ListfileRequest selects exactly one listfile source. Offline forbids network
// and fails precisely when no verified cache exists; Refresh re-downloads the
// same source and never falls back to another one.
type ListfileRequest struct {
	Kind    ListfileKind
	URLs    []string
	Fetch   ListfileFetcher
	Offline bool
	Refresh bool
	Limits  ListfileLimits
}

// ListfileReading is one prepared listfile with the provenance that produced it.
type ListfileReading struct {
	Index      *ListfileIndex
	Provenance ListfileProvenance
}

const listfileCacheSchema = "lycheedev.listfile-source.v1"

type listfileCacheDocument struct {
	Schema     string             `json:"schema"`
	Provenance ListfileProvenance `json:"provenance"`
}

func listfileCacheKey(kind ListfileKind) string { return "listfile/" + string(kind) }

// PrepareListfile parses the requested community listfile source into one
// lookup structure, caching the exact downloaded bytes in the managed
// workspace with source, fetch time and entry count. A verified cache is
// reused without network; offline never downloads and never substitutes
// another source when the requested one is missing.
func PrepareListfile(ctx context.Context, root string, request ListfileRequest) (ListfileReading, error) {
	if err := ctx.Err(); err != nil {
		return ListfileReading{}, err
	}
	if err := validateListfileRequest(request); err != nil {
		return ListfileReading{}, err
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (ListfileReading, error) {
		if request.Offline {
			reading, cacheErr := loadCachedListfile(ctx, s, m, request)
			if cacheErr != nil {
				if errors.Is(cacheErr, vault.ErrMissingRecord) {
					return ListfileReading{}, fmt.Errorf("%w: no %s listfile cached in this workspace", ErrListfileUnavailable, request.Kind)
				}
				return ListfileReading{}, fmt.Errorf("%w: cached %s listfile is unusable: %v", ErrListfileUnavailable, request.Kind, cacheErr)
			}
			return reading, nil
		}
		if !request.Refresh {
			if reading, cacheErr := loadCachedListfile(ctx, s, m, request); cacheErr == nil {
				return reading, nil
			}
			// An unusable cache falls through to re-fetching the same explicit
			// source; it never switches to another listfile kind or origin.
		}
		return fetchListfile(ctx, s, m, request)
	})
}

func validateListfileRequest(request ListfileRequest) error {
	if request.Kind != ListfileCommunityCSV && request.Kind != ListfileWowExportText && request.Kind != ListfileWowExportBinary {
		return ErrListfileQuery
	}
	if request.Offline && request.Refresh {
		return ErrListfileQuery
	}
	if _, err := normalizeListfileLimits(request.Limits); err != nil {
		return err
	}
	return nil
}

func loadCachedListfile(ctx context.Context, s *vault.Store, m *vault.Metadata, request ListfileRequest) (ListfileReading, error) {
	limits, err := normalizeListfileLimits(request.Limits)
	if err != nil {
		return ListfileReading{}, err
	}
	raw, err := m.ReadDocument(ctx, listfileCacheKey(request.Kind))
	if err != nil {
		return ListfileReading{}, err
	}
	var document listfileCacheDocument
	if err := json.Unmarshal(raw.Value, &document); err != nil || document.Schema != listfileCacheSchema || document.Provenance.Kind != request.Kind {
		return ListfileReading{}, ErrListfileFormat
	}
	payloads := make(map[string][]byte, len(document.Provenance.Files))
	var total int64
	for _, file := range document.Provenance.Files {
		blob, err := s.ReadBlob(ctx, file.Blob, limits.Bytes)
		if err != nil {
			return ListfileReading{}, err
		}
		digest := sha256.Sum256(blob)
		if hex.EncodeToString(digest[:]) != file.SHA256 || int64(len(blob)) != file.Bytes {
			return ListfileReading{}, fmt.Errorf("%w: cached payload %s failed digest check", ErrListfileFormat, file.Component)
		}
		total += int64(len(blob))
		payloads[file.Component] = blob
	}
	if total != document.Provenance.TotalBytes {
		return ListfileReading{}, fmt.Errorf("%w: cached payload sizes changed", ErrListfileFormat)
	}
	index, err := parseListfilePayloads(request.Kind, limits, payloads)
	if err != nil {
		return ListfileReading{}, err
	}
	if index.provenance.EntryCount != document.Provenance.EntryCount || index.provenance.Rejected != document.Provenance.Rejected {
		return ListfileReading{}, fmt.Errorf("%w: cached entry counts changed", ErrListfileFormat)
	}
	index.provenance = document.Provenance
	index.provenance.Cache = "verified-cache"
	if request.Offline {
		index.provenance.Cache = "offline-cache"
	}
	return ListfileReading{Index: index, Provenance: index.provenance}, nil
}

func fetchListfile(ctx context.Context, s *vault.Store, m *vault.Metadata, request ListfileRequest) (ListfileReading, error) {
	if request.Fetch == nil {
		return ListfileReading{}, fmt.Errorf("%w: no fetcher for %s", ErrListfileUnavailable, request.Kind)
	}
	limits, err := normalizeListfileLimits(request.Limits)
	if err != nil {
		return ListfileReading{}, err
	}
	urls := request.URLs
	if len(urls) == 0 {
		urls = ListfileSourceSpec(request.Kind)
	}
	if len(urls) == 0 {
		return ListfileReading{}, fmt.Errorf("%w: no URLs for %s", ErrListfileUnavailable, request.Kind)
	}
	components := []string{""}
	if request.Kind == ListfileWowExportBinary {
		components = BinaryListfileComponents()
	}
	payloads := make(map[string][]byte, len(components))
	var files []ListfileSourceFile
	var lastErr error
	for _, component := range components {
		fetched := false
		for _, template := range urls {
			url := template
			if component != "" {
				url = fmt.Sprintf(template, component)
			}
			body, err := request.Fetch.FetchListfile(ctx, url, limits.Bytes)
			if err != nil {
				lastErr = err
				continue
			}
			if int64(len(body)) > limits.Bytes {
				lastErr = ErrListfileLimit
				continue
			}
			name := component
			if name == "" {
				name = "listfile"
			}
			payloads[name] = body
			digest := sha256.Sum256(body)
			ref, err := s.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(body), MaxBytes: limits.Bytes})
			if err != nil {
				return ListfileReading{}, err
			}
			files = append(files, ListfileSourceFile{
				Component: name,
				URL:       url,
				Bytes:     int64(len(body)),
				SHA256:    hex.EncodeToString(digest[:]),
				Blob:      ref,
			})
			fetched = true
			break
		}
		if !fetched {
			if lastErr == nil {
				lastErr = errors.New("no source URL succeeded")
			}
			return ListfileReading{}, fmt.Errorf("%w: %s listfile component %q: %v", ErrListfileUnavailable, request.Kind, component, lastErr)
		}
	}
	index, err := parseListfilePayloads(request.Kind, limits, payloads)
	if err != nil {
		return ListfileReading{}, err
	}
	index.provenance.Files = files
	index.provenance.FetchedAt = time.Now().UTC()
	index.provenance.Cache = "fetched"
	document := listfileCacheDocument{Schema: listfileCacheSchema, Provenance: index.provenance}
	encoded, err := json.Marshal(document)
	if err != nil {
		return ListfileReading{}, err
	}
	// Cache replacement is an explicit compare-and-swap on the stored
	// provenance document, never a blind overwrite.
	var generation int64
	if existing, err := m.ReadDocument(ctx, listfileCacheKey(request.Kind)); err == nil {
		generation = existing.Generation
	} else if !errors.Is(err, vault.ErrMissingRecord) {
		return ListfileReading{}, err
	}
	if err := m.CommitDocuments(ctx, vault.Mutation{Key: listfileCacheKey(request.Kind), Value: encoded, ExpectedGeneration: generation}); err != nil {
		return ListfileReading{}, err
	}
	return ListfileReading{Index: index, Provenance: index.provenance}, nil
}

// parseListfilePayloads keys text sources under the "listfile" component and
// binary sources under their component file names.
func parseListfilePayloads(kind ListfileKind, limits ListfileLimits, payloads map[string][]byte) (*ListfileIndex, error) {
	if kind == ListfileWowExportBinary {
		return ParseBinaryListfile(payloads, limits)
	}
	body, ok := payloads["listfile"]
	if !ok {
		return nil, fmt.Errorf("%w: missing text payload", ErrListfileFormat)
	}
	return ParseTextListfile(kind, body, limits)
}
