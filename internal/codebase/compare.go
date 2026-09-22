package codebase

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"sort"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type SourcePair struct{ From, To selection.SourcePin }
type ChangeQuery struct{ Limit int }
type DocumentChange struct {
	Path   string         `json:"path"`
	Status string         `json:"status"`
	Before *vault.BlobRef `json:"before,omitempty"`
	After  *vault.BlobRef `json:"after,omitempty"`
}
type DeclarationSite struct {
	Path      string `json:"path"`
	Line      int    `json:"line"`
	EndLine   int    `json:"endLine"`
	Signature string `json:"signature"`
}
type DeclarationChange struct {
	Name     string            `json:"name"`
	Category string            `json:"category"`
	Status   string            `json:"status"`
	Before   []DeclarationSite `json:"before"`
	After    []DeclarationSite `json:"after"`
}
type SourceDelta struct {
	From               IndexSummary        `json:"from"`
	To                 IndexSummary        `json:"to"`
	Documents          []DocumentChange    `json:"documents"`
	Declarations       []DeclarationChange `json:"declarations"`
	DocumentChanges    int                 `json:"documentChanges"`
	DeclarationChanges int                 `json:"declarationChanges"`
	Truncated          bool                `json:"truncated"`
}

// CompareTrees compares indexed source observations. It preserves duplicate
// declaration sites and never interprets a moved line as runtime incompatibility.
// Incomplete syntax extraction cannot prove the absence of a declaration.
func (b *Browser) CompareTrees(ctx context.Context, pair SourcePair, query ChangeQuery) (SourceDelta, error) {
	result := SourceDelta{Documents: []DocumentChange{}, Declarations: []DeclarationChange{}}
	if query.Limit < 1 || query.Limit > 200 || pair.From.Repository != pair.To.Repository || pair.From.ParserRevision != pair.To.ParserRevision {
		return result, errors.New("codebase.incompatible_comparison")
	}
	left, from, err := b.openIndex(ctx, pair.From)
	if err != nil {
		return result, err
	}
	defer left.Close()
	right, to, err := b.openIndex(ctx, pair.To)
	if err != nil {
		return result, err
	}
	defer right.Close()
	result.From, result.To = from, to
	if !from.Complete || !to.Complete {
		return result, errors.New("codebase.incomplete_index: inspect source index diagnostics before comparing declarations")
	}
	before, err := indexedDocuments(ctx, left)
	if err != nil {
		return result, err
	}
	after, err := indexedDocuments(ctx, right)
	if err != nil {
		return result, err
	}
	paths := make(map[string]bool, len(before)+len(after))
	for key := range before {
		paths[key] = true
	}
	for key := range after {
		paths[key] = true
	}
	ordered := make([]string, 0, len(paths))
	for key := range paths {
		ordered = append(ordered, key)
	}
	sort.Strings(ordered)
	for _, path := range ordered {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		a, hasA := before[path]
		z, hasZ := after[path]
		if hasA && hasZ && a == z {
			continue
		}
		result.DocumentChanges++
		if len(result.Documents) == query.Limit {
			result.Truncated = true
			continue
		}
		change := DocumentChange{Path: path, Status: "changed"}
		if hasA {
			change.Before = &a
		} else {
			change.Status = "added"
		}
		if hasZ {
			change.After = &z
		} else {
			change.Status = "removed"
		}
		result.Documents = append(result.Documents, change)
	}
	oldSites, err := indexedDeclarations(ctx, left)
	if err != nil {
		return result, err
	}
	newSites, err := indexedDeclarations(ctx, right)
	if err != nil {
		return result, err
	}
	keys := make(map[declarationKey]bool, len(oldSites)+len(newSites))
	for key := range oldSites {
		keys[key] = true
	}
	for key := range newSites {
		keys[key] = true
	}
	names := make([]declarationKey, 0, len(keys))
	for key := range keys {
		names = append(names, key)
	}
	sort.Slice(names, func(i, j int) bool {
		if names[i].name == names[j].name {
			return names[i].category < names[j].category
		}
		return names[i].name < names[j].name
	})
	for _, key := range names {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		a, hasA := oldSites[key]
		z, hasZ := newSites[key]
		if slices.Equal(a, z) {
			continue
		}
		result.DeclarationChanges++
		if len(result.Declarations) == query.Limit {
			result.Truncated = true
			continue
		}
		change := DeclarationChange{Name: key.name, Category: key.category, Status: "changed", Before: a, After: z}
		if !hasA {
			change.Status = "added"
			change.Before = []DeclarationSite{}
		}
		if !hasZ {
			change.Status = "removed"
			change.After = []DeclarationSite{}
		}
		result.Declarations = append(result.Declarations, change)
	}
	return result, nil
}

func indexedDocuments(ctx context.Context, db *sql.DB) (map[string]vault.BlobRef, error) {
	rows, err := db.QueryContext(ctx, "SELECT path,sha256,bytes FROM documents ORDER BY path LIMIT 100001")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := map[string]vault.BlobRef{}
	for rows.Next() {
		var path string
		var ref vault.BlobRef
		if err := rows.Scan(&path, &ref.SHA256, &ref.Bytes); err != nil {
			return nil, err
		}
		if len(result) >= 100000 {
			return nil, errors.New("codebase.diff_document_budget")
		}
		result[path] = ref
	}
	return result, rows.Err()
}

type declarationKey struct{ name, category string }

func indexedDeclarations(ctx context.Context, db *sql.DB) (map[declarationKey][]DeclarationSite, error) {
	rows, err := db.QueryContext(ctx, "SELECT name,category,path,line,end_line,signature FROM entries WHERE kind='declaration' ORDER BY name,category,path,line,end_line,signature LIMIT 500001")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := map[declarationKey][]DeclarationSite{}
	count := 0
	for rows.Next() {
		var key declarationKey
		var site DeclarationSite
		if err := rows.Scan(&key.name, &key.category, &site.Path, &site.Line, &site.EndLine, &site.Signature); err != nil {
			return nil, err
		}
		count++
		if count > 500000 {
			return nil, errors.New("codebase.diff_declaration_budget")
		}
		result[key] = append(result[key], site)
	}
	return result, rows.Err()
}
