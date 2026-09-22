package codebase

import (
	"context"
	"database/sql"
	"errors"
	"path"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

// TargetQuery selects one inspection target: a qualified symbol or a
// repository path. Exactly one must be provided.
type TargetQuery struct {
	Symbol string `json:"symbol"`
	Path   string `json:"path"`
}

const inspectTargetLimit = 25

// InspectTarget inspects symbols and paths of one pinned snapshot. A symbol
// that names an exact indexed file inspects that file; any other symbol falls
// back to a precise search bounded to 25 results. Path targets return file
// rows enriched with asset metadata rows (format/mime/bytes/width/height/local
// where derivable).
func (b *Browser) InspectTarget(ctx context.Context, snapshotID string, pin selection.SourcePin, query TargetQuery) (SearchResponse, error) {
	symbol := strings.TrimSpace(query.Symbol)
	target := strings.TrimSpace(query.Path)
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, MatchedTag: nil, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
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
			// Not an exact file path: fall back to the precise search engine.
			return b.Search(ctx, snapshotID, pin, SearchQuery{Mode: SearchModePrecise, Text: symbol, Limit: inspectTargetLimit})
		}
		target = exact
	}
	return b.inspectPath(ctx, snapshotID, pin, target)
}

func (b *Browser) inspectPath(ctx context.Context, snapshotID string, pin selection.SourcePin, target string) (SearchResponse, error) {
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, MatchedTag: nil, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
	if len(target) > 4096 {
		return response, errors.New("codebase.invalid_span")
	}
	db, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return response, err
	}
	defer db.Close()
	pattern := "%" + escapeLike(target) + "%"
	rows, err := db.QueryContext(ctx, `SELECT path,sha256,bytes FROM documents WHERE path=? OR lower(path) LIKE lower(?) ESCAPE '\' ORDER BY CASE WHEN path=? THEN 0 ELSE 1 END,path LIMIT ?`,
		target, pattern, target, inspectTargetLimit+1)
	if err != nil {
		return response, err
	}
	type fileRow struct{ path, hash string }
	files := []fileRow{}
	for rows.Next() {
		var row fileRow
		var size int64
		if err := rows.Scan(&row.path, &row.hash, &size); err != nil {
			rows.Close()
			return response, err
		}
		if len(files) == inspectTargetLimit {
			response.Truncated = true
			break
		}
		files = append(files, row)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return response, err
	}
	rows.Close()
	excerpts := newExcerptReader(ctx, b, db)
	for _, row := range files {
		_, excerpt, err := excerpts.lines(row.path, 1, 6, 0)
		if err != nil {
			return response, err
		}
		response.Results = append(response.Results, Match{Kind: "file", Name: path.Base(row.path), Path: row.path, Line: 1, MatchedBy: "path", Role: pathRole(row.path),
			Score: 100, ScoreParts: map[string]int{"match": 100}, ContentHash: row.hash, Excerpt: excerpt})
	}
	if requireAssetIndex(summary) == nil {
		assetRows, err := db.QueryContext(ctx, `SELECT path,sha256,bytes,extension,mime,format,width,height FROM assets WHERE path=? OR lower(path) LIKE lower(?) ESCAPE '\' ORDER BY CASE WHEN path=? THEN 0 ELSE 1 END,path LIMIT ?`,
			target, pattern, target, inspectTargetLimit+1)
		if err != nil {
			return response, err
		}
		defer assetRows.Close()
		for assetRows.Next() {
			var asset AssetRow
			if err := assetRows.Scan(&asset.Path, &asset.SHA256Stored, &asset.Bytes, &asset.Extension, &asset.MIME, &asset.Format, &asset.Width, &asset.Height); err != nil {
				return response, err
			}
			if len(response.Results) >= inspectTargetLimit {
				response.Truncated = true
				break
			}
			asset.Local = b.localBlobPath(asset.SHA256Stored)
			response.Results = append(response.Results, Match{Kind: "asset", Name: path.Base(asset.Path), Path: asset.Path, Line: 1, MatchedBy: "path", Role: "project",
				Score: 100, ContentHash: asset.SHA256Stored, Excerpt: assetMetaExcerpt(asset)})
		}
		if err := assetRows.Err(); err != nil {
			return response, err
		}
	} else {
		response.Suggestions = append(response.Suggestions, "asset rows need the current index schema; rebuild the source index")
	}
	response.Complete = !response.Truncated
	if len(response.Results) == 0 {
		response.Suggestions = []string{"use query --topic asset with a filename or a shorter path"}
		response.Complete = true
	}
	return response, nil
}

// exactIndexedPath returns the canonical indexed path when the symbol names an
// exact document or asset path, and "" otherwise.
func (b *Browser) exactIndexedPath(ctx context.Context, pin selection.SourcePin, symbol string) (string, error) {
	if !sourcePath(symbol) {
		return "", nil
	}
	db, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return "", err
	}
	defer db.Close()
	var found string
	err = db.QueryRowContext(ctx, "SELECT path FROM documents WHERE path=?", symbol).Scan(&found)
	if err == nil {
		return found, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return "", err
	}
	if requireAssetIndex(summary) == nil {
		err = db.QueryRowContext(ctx, "SELECT path FROM assets WHERE path=?", symbol).Scan(&found)
		if err == nil {
			return found, nil
		}
		if !errors.Is(err, sql.ErrNoRows) {
			return "", err
		}
	}
	return "", nil
}

// localBlobPath derives the content-addressed blob location in the workspace
// store, matching vault's blob layout. Empty when no content identity exists.
func (b *Browser) localBlobPath(digest string) string {
	if len(digest) != 64 {
		return ""
	}
	return filepath.Join(b.store.Root(), "blobs", digest[:2], digest[2:])
}
