package codebase

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type TargetQuery struct {
	Symbol string `json:"symbol"`
	Path   string `json:"path"`
}

const inspectTargetLimit = 25

func (b *Browser) InspectTarget(ctx context.Context, snapshotID string, pin selection.SourcePin, query TargetQuery) (SearchResponse, error) {
	symbol, target := strings.TrimSpace(query.Symbol), strings.TrimSpace(query.Path)
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
	if symbol == "" && target == "" {
		return response, errors.New("codebase.inspect_target_required")
	}
	if symbol != "" && target != "" {
		return response, errors.New("codebase.inspect_target_conflict")
	}
	if symbol != "" {
		exact, err := b.exactIndexedPath(ctx, pin, symbol)
		if err != nil {
			return response, err
		}
		if exact == "" {
			return b.Search(ctx, snapshotID, pin, SearchQuery{Mode: SearchModePrecise, Text: symbol, Limit: inspectTargetLimit})
		}
		target = exact
	}
	return b.inspectPath(ctx, snapshotID, pin, target)
}

func (b *Browser) inspectPath(ctx context.Context, snapshotID string, pin selection.SourcePin, target string) (SearchResponse, error) {
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
	if len(target) > 4096 {
		return response, errors.New("codebase.invalid_span")
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return response, err
	}
	var files, assets []sourceRecord
	documents := map[string]sourceRecord{}
	assetRows := map[string]sourceRecord{}
	probe := strings.ToLower(target)
	err = cache.scan(ctx, func(r sourceRecord) error {
		if r.Kind == "document" {
			documents[r.Path] = r
		}
		if r.Kind == "asset" {
			assetRows[r.Path] = r
		}
		if (r.Kind == "document" || r.Kind == "asset") && strings.Contains(strings.ToLower(r.Path), probe) {
			if r.Kind == "asset" {
				assets = append(assets, r)
			} else {
				files = append(files, r)
			}
		}
		return nil
	})
	if err != nil {
		return response, err
	}
	cache.documents = documents
	cache.assets = assetRows
	cache.pathsOnce.Do(func() {})
	less := func(rows []sourceRecord) {
		sort.Slice(rows, func(i, j int) bool {
			a, z := rows[i].Path, rows[j].Path
			if a == target {
				return true
			}
			if z == target {
				return false
			}
			return a < z
		})
	}
	less(files)
	less(assets)
	for _, r := range files {
		if len(response.Results) >= inspectTargetLimit {
			response.Truncated = true
			break
		}
		data, hash, err := cache.document(ctx, r.Path)
		if err != nil {
			return response, err
		}
		response.Results = append(response.Results, Match{Kind: "file", Name: path.Base(r.Path), Path: r.Path, Line: 1, MatchedBy: "path", Role: pathRole(r.Path), Score: 100, ScoreParts: map[string]int{"match": 100}, ContentHash: hash, Excerpt: numberedLines(data, 1, 6, 0)})
	}
	for _, r := range assets {
		if len(response.Results) >= inspectTargetLimit {
			response.Truncated = true
			break
		}
		asset := *r.Asset
		if asset.ContentHash != "" {
			data, err := cache.assetBytes(ctx, r)
			if err != nil {
				return response, err
			}
			ref, err := b.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(data), MaxBytes: maxAssetBytes, ExpectedSHA256: asset.ContentHash})
			if err != nil {
				return response, err
			}
			asset.Local = b.localBlobPath(ref.SHA256)
		}
		response.Results = append(response.Results, Match{Kind: "asset", Name: path.Base(asset.Path), Path: asset.Path, Line: 1, MatchedBy: "path", Role: "project", Score: 100, ContentHash: asset.ContentHash, Excerpt: assetMetaExcerpt(asset)})
	}
	response.Complete = !response.Truncated
	if len(response.Results) == 0 {
		response.Suggestions = []string{"use query --topic asset with a filename or a shorter path"}
	}
	return response, nil
}

func (c *snapshotCache) assetBytes(ctx context.Context, r sourceRecord) ([]byte, error) {
	if r.Kind != "asset" || r.Asset == nil || r.Asset.ContentHash == "" || r.Bytes < 0 || r.Bytes > maxAssetBytes {
		return nil, errors.New("codebase.asset_not_archivable")
	}
	var data []byte
	var err error
	if c.manifest.FixtureRoot != "" {
		f, openErr := os.Open(filepath.Join(c.manifest.FixtureRoot, filepath.FromSlash(r.Path)))
		if openErr != nil {
			return nil, openErr
		}
		data, err = io.ReadAll(io.LimitReader(f, r.Bytes+1))
		closeErr := f.Close()
		if err != nil || closeErr != nil {
			return nil, errors.Join(err, closeErr)
		}
	} else {
		data, err = gitBytes(ctx, c.b.mirror(c.pin.Repository), int(r.Bytes)+1, "cat-file", "blob", r.Object)
		if err != nil {
			return nil, err
		}
	}
	sum := sha256.Sum256(data)
	if int64(len(data)) != r.Bytes || hex.EncodeToString(sum[:]) != r.Asset.ContentHash {
		return nil, errors.New("codebase.asset_content_mismatch")
	}
	return data, nil
}

func (b *Browser) exactIndexedPath(ctx context.Context, pin selection.SourcePin, symbol string) (string, error) {
	if !sourcePath(symbol) {
		return "", nil
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return "", err
	}
	found := ""
	err = cache.scan(ctx, func(r sourceRecord) error {
		if (r.Kind == "document" || r.Kind == "asset") && r.Path == symbol {
			found = r.Path
		}
		return nil
	})
	return found, err
}
func (b *Browser) localBlobPath(digest string) string {
	if len(digest) != 64 {
		return ""
	}
	path := filepath.Join(b.store.Root(), "blobs", digest[:2], digest[2:])
	if _, err := filepath.Abs(path); err != nil {
		return ""
	}
	return path
}
