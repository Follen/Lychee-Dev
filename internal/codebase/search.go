package codebase

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// SearchMode selects one documented tier-escalation behavior over the same
// search engine. Precise stops at the strongest evidence tier with hits;
// exploratory always includes weaker tiers and OR-joins full-text terms.
type SearchMode string

const (
	SearchModePrecise     SearchMode = "precise"
	SearchModeExploratory SearchMode = "exploratory"
)

// SearchQuery is the bounded query contract of the source search engine.
// Topic is one of "", "api", "lua", "xml", "toc", or "asset".
type SearchQuery struct {
	Mode  SearchMode `json:"mode"`
	Text  string     `json:"text"`
	Topic string     `json:"topic"`
	Limit int        `json:"limit"`
}

// Match keeps the legacy result field names relied on by callers.
type Match struct {
	Kind        string         `json:"kind"`
	Name        string         `json:"name,omitempty"`
	Path        string         `json:"path"`
	MatchedBy   string         `json:"matchedBy"`
	Role        string         `json:"role"`
	Confidence  string         `json:"confidence,omitempty"`
	Excerpt     string         `json:"excerpt"`
	ContentHash string         `json:"contentHash"`
	Line        int            `json:"line"`
	Score       int            `json:"score"`
	ScoreParts  map[string]int `json:"scoreParts,omitempty"`
}

type Relation struct {
	Source     string `json:"source,omitempty"`
	Target     string `json:"target"`
	Kind       string `json:"kind"`
	Confidence string `json:"confidence"`
	Path       string `json:"path"`
	Line       int    `json:"line"`
}

type SearchResponse struct {
	SourceID       string     `json:"sourceId"`
	Product        string     `json:"product"`
	RequestedRef   string     `json:"requestedRef,omitempty"`
	MatchedTag     any        `json:"matchedTag"`
	ResolvedCommit string     `json:"resolvedCommit"`
	SnapshotID     string     `json:"snapshotId"`
	Results        []Match    `json:"results"`
	Relations      []Relation `json:"relations,omitempty"`
	Suggestions    []string   `json:"suggestions,omitempty"`
	// Complete and Truncated report result-bound honesty: a limit reached
	// with more distinct evidence pending is truncated, never silent.
	Complete  bool `json:"complete"`
	Truncated bool `json:"truncated"`
}

type searchCandidate struct {
	kind, name, target, category, confidence, path, matched, signature string
	line, endLine, rank                                                int
}

// rolePenaltySQL mirrors roleRankPenalty so candidate filtering and ranking
// happen before LIMIT, exactly like the legacy pre-limit semantics.
const rolePenaltySQL = `(CASE
 WHEN instr(lower(path),'apidocumentationgenerated')>0 THEN 0
 WHEN instr(lower(path),'locale')>0 OR instr(lower(path),'localization')>0 THEN 20
 WHEN instr(lower(path),'libs/')>0 OR instr(lower(path),'vendor/')>0 THEN 15
 WHEN instr(lower(path),'modelpaths')>0 OR instr(lower(path),'generated')>0 THEN 20
 WHEN instr(lower(path),'tools/')>0 OR lower(path) LIKE '%babelfish.lua' THEN 20
 ELSE 0 END)`

const candidateColumns = "matched,rank,kind,name,target,category,confidence,path,line,end_line,signature"

// Search runs the single tier-escalation engine over one immutable index. The
// matchedBy values (exact_symbol, exact_fact, symbol_prefix, name_prefix,
// fts5, asset_path) are stable tier identifiers; the fts5 tier is evaluated as
// bounded token matching over indexed names, targets and signatures.
func (b *Browser) Search(ctx context.Context, snapshotID string, pin selection.SourcePin, query SearchQuery) (SearchResponse, error) {
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, MatchedTag: nil, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
	text := strings.TrimSpace(query.Text)
	if text == "" || len(text) > 512 {
		return response, errors.New("codebase.query_required")
	}
	topic := strings.ToLower(strings.TrimSpace(query.Topic))
	filter, err := searchTopicFilter(topic)
	if err != nil {
		return response, err
	}
	limit := query.Limit
	if limit <= 0 {
		limit = 10
	}
	if limit > 200 {
		return response, errors.New("codebase.invalid_search_limit")
	}
	if query.Mode != "" && query.Mode != SearchModePrecise && query.Mode != SearchModeExploratory {
		return response, errors.New("codebase.invalid_search_mode")
	}
	broad := query.Mode == SearchModeExploratory
	db, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return response, err
	}
	defer db.Close()

	var candidates []searchCandidate
	collect := func(statement string, args ...any) error {
		statement = `SELECT ` + candidateColumns + ` FROM (` + statement + `) WHERE ` + filter +
			` ORDER BY rank-` + rolePenaltySQL + ` DESC,path,line,kind,name LIMIT ?`
		args = append(args, limit*3)
		rows, e := db.QueryContext(ctx, statement, args...)
		if e != nil {
			return e
		}
		defer rows.Close()
		for rows.Next() {
			var c searchCandidate
			if e = rows.Scan(&c.matched, &c.rank, &c.kind, &c.name, &c.target, &c.category, &c.confidence, &c.path, &c.line, &c.endLine, &c.signature); e != nil {
				return e
			}
			candidates = append(candidates, c)
		}
		return rows.Err()
	}
	if topic != "asset" {
		escaped := escapeLike(text)
		exact := `SELECT 'exact_symbol' AS matched,100 AS rank,'declaration' AS kind,name,target,category,confidence,path,line,end_line,signature FROM entries WHERE kind='declaration' AND (name=? OR target=? OR name LIKE ? ESCAPE '\')
UNION ALL
SELECT 'exact_fact',90,'header',name,target,category,confidence,path,line,end_line,signature FROM entries WHERE kind='header' AND name=?`
		if err = collect(exact, text, text, "%."+escaped, text); err != nil {
			return response, err
		}
		// Precise mode escalates only while no candidate exists; exploratory
		// mode always includes the weaker tiers.
		if broad || len(candidates) == 0 {
			prefix := `SELECT 'symbol_prefix' AS matched,80 AS rank,'declaration' AS kind,name,target,category,confidence,path,line,end_line,signature FROM entries WHERE kind='declaration' AND (name LIKE ? ESCAPE '\' OR name LIKE ? ESCAPE '\')
UNION ALL
SELECT 'name_prefix',80,'header',name,target,category,confidence,path,line,end_line,signature FROM entries WHERE kind='header' AND name LIKE ? ESCAPE '\'`
			if err = collect(prefix, escaped+"%", "%."+escaped+"%", escaped+"%"); err != nil {
				return response, err
			}
		}
		if broad || len(candidates) == 0 {
			fts, ftsArgs := fullTextTier(text, broad)
			if err = collect(fts, ftsArgs...); err != nil {
				return response, err
			}
		}
	}
	if topic == "asset" {
		if err = requireAssetIndex(summary); err != nil {
			return response, err
		}
		normalized := normalizeAssetPath(text)
		assetSQL := `SELECT 'asset_path' AS matched,CASE WHEN normalized_path=? THEN 100 ELSE 70 END AS rank,'asset' AS kind,path AS name,'' AS target,'asset' AS category,'exact' AS confidence,path,1 AS line,0 AS end_line,'' AS signature FROM assets WHERE normalized_path LIKE ? ESCAPE '\'`
		if err = collect(assetSQL, normalized, "%"+escapeLike(normalized)+"%"); err != nil {
			return response, err
		}
	}
	sort.SliceStable(candidates, func(i, j int) bool {
		left := candidates[i].rank - roleRankPenalty(pathRole(candidates[i].path))
		right := candidates[j].rank - roleRankPenalty(pathRole(candidates[j].path))
		return left > right
	})
	excerpts := newExcerptReader(ctx, b, db)
	seen := map[string]bool{}
	for _, c := range candidates {
		key := fmt.Sprintf("%s:%d\x00%s", c.path, c.line, c.name)
		if seen[key] {
			continue
		}
		seen[key] = true
		if len(response.Results) >= limit {
			response.Truncated = true
			break
		}
		role := pathRole(c.path)
		penalty := roleRankPenalty(role)
		var hash, snippet string
		if c.kind == "asset" {
			hash, err = assetContentHash(ctx, db, c.path)
			if err != nil {
				return response, err
			}
			snippet = c.path
		} else if c.matched == "exact_symbol" && c.endLine >= c.line {
			hash, snippet, err = excerpts.lines(c.path, c.line, c.endLine, 80)
		} else {
			hash, snippet, err = excerpts.lines(c.path, max(c.line-3, 1), c.line+3, 0)
		}
		if err != nil {
			return response, err
		}
		// Header rows surface as legacy "toc" facts; declaration rows carry
		// their declaration category as the result kind.
		kind := c.category
		if c.kind == "header" {
			kind = "toc"
		}
		response.Results = append(response.Results, Match{Kind: kind, Name: c.name, Path: c.path, Line: c.line, MatchedBy: c.matched, Role: role,
			Score: c.rank - penalty, ScoreParts: map[string]int{"match": c.rank, "rolePenalty": -penalty}, ContentHash: hash, Excerpt: snippet})
	}
	// Relations are exact by default and share the requested bound. Broad
	// substring relations belong to exploratory mode and never bypass the
	// limit.
	relationsCut, err := searchRelations(ctx, db, text, limit, broad, &response)
	if err != nil {
		return response, err
	}
	response.Truncated = response.Truncated || relationsCut
	response.Complete = summary.Complete && !response.Truncated
	if len(response.Results) == 0 {
		response.Suggestions = []string{"use explore for broader text and symbol matches", "check the topic, product and ref"}
	}
	return response, nil
}

func searchTopicFilter(topic string) (string, error) {
	switch topic {
	case "":
		return "1=1", nil
	case "api":
		return "category LIKE 'api-%'", nil
	case "lua", "xml", "toc":
		return "lower(path) LIKE '%." + topic + "'", nil
	case "asset":
		return "kind='asset'", nil
	default:
		return "", errors.New("codebase.invalid_topic: use api, lua, xml, toc, or asset")
	}
}

// fullTextTier builds the bounded token-matching tier. Precise mode requires
// every term (legacy AND semantics); exploratory mode OR-joins the terms.
func fullTextTier(text string, broad bool) (string, []any) {
	tokens := strings.Fields(strings.ToLower(text))
	if len(tokens) == 0 {
		tokens = []string{strings.ToLower(text)}
	}
	haystack := "lower(name||' '||target||' '||signature)"
	matched := make([]string, 0, len(tokens))
	predicate := make([]string, 0, len(tokens))
	for range tokens {
		probe := "instr(" + haystack + ",?)"
		matched = append(matched, "CASE WHEN "+probe+">0 THEN 1 ELSE 0 END")
		predicate = append(predicate, probe+">0")
	}
	joiner := " AND "
	if broad {
		joiner = " OR "
	}
	statement := `SELECT 'fts5' AS matched,(70+MIN(9,` + strings.Join(matched, "+") + `)) AS rank,kind,name,target,category,confidence,path,line,end_line,signature FROM entries WHERE kind IN ('declaration','header') AND (` + strings.Join(predicate, joiner) + `)`
	args := make([]any, 0, len(tokens)*2)
	for _, token := range tokens {
		args = append(args, token)
	}
	for _, token := range tokens {
		args = append(args, token)
	}
	return statement, args
}

func escapeLike(text string) string {
	return strings.NewReplacer(`\`, `\\`, "%", `\%`, `_`, `\_`).Replace(text)
}

func searchRelations(ctx context.Context, db *sql.DB, text string, limit int, broad bool, response *SearchResponse) (bool, error) {
	where := `(name=? OR target=?)`
	args := []any{text, text}
	if broad {
		where = `(name LIKE ? ESCAPE '\' OR target LIKE ? ESCAPE '\')`
		escaped := "%" + escapeLike(text) + "%"
		args = []any{escaped, escaped}
	}
	statement := `SELECT name,target,category,confidence,path,line FROM entries WHERE kind='relationship' AND ` + where +
		` ORDER BY CASE confidence WHEN 'exact' THEN 0 WHEN 'inferred' THEN 1 ELSE 2 END,path,line LIMIT ?`
	args = append(args, limit+1)
	rows, err := db.QueryContext(ctx, statement, args...)
	if err != nil {
		return false, err
	}
	defer rows.Close()
	truncated := false
	for rows.Next() {
		var relation Relation
		if err := rows.Scan(&relation.Source, &relation.Target, &relation.Kind, &relation.Confidence, &relation.Path, &relation.Line); err != nil {
			return false, err
		}
		if len(response.Relations) >= limit {
			truncated = true
			break
		}
		if relation.Source == "<file>" {
			relation.Source = relation.Path
		}
		response.Relations = append(response.Relations, relation)
	}
	return truncated, rows.Err()
}

// excerptReader returns legacy line-numbered excerpts from archived document
// bytes, reading each document at most once per query.
type excerptReader struct {
	ctx     context.Context
	browser *Browser
	db      *sql.DB
	blobs   map[string][]byte
	hashes  map[string]string
}

func newExcerptReader(ctx context.Context, b *Browser, db *sql.DB) *excerptReader {
	return &excerptReader{ctx: ctx, browser: b, db: db, blobs: map[string][]byte{}, hashes: map[string]string{}}
}

func (r *excerptReader) document(documentPath string) ([]byte, string, error) {
	if data, ok := r.blobs[documentPath]; ok {
		return data, r.hashes[documentPath], nil
	}
	var ref vault.BlobRef
	if err := r.db.QueryRowContext(r.ctx, "SELECT sha256,bytes FROM documents WHERE path=?", documentPath).Scan(&ref.SHA256, &ref.Bytes); err != nil {
		return nil, "", err
	}
	data, err := r.browser.store.ReadBlob(r.ctx, ref, maxSourceBytes)
	if err != nil {
		return nil, "", err
	}
	r.blobs[documentPath] = data
	r.hashes[documentPath] = ref.SHA256
	return data, ref.SHA256, nil
}

func (r *excerptReader) lines(documentPath string, start, end, maxLines int) (string, string, error) {
	data, hash, err := r.document(documentPath)
	if err != nil {
		return "", "", err
	}
	if end < start {
		end = start
	}
	if maxLines > 0 && end-start+1 > maxLines {
		end = start + maxLines - 1
	}
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	out := []string{}
	for n := start; n <= end && n <= len(lines); n++ {
		out = append(out, fmt.Sprintf("%d: %s", n, strings.TrimSuffix(lines[n-1], "\r")))
	}
	return hash, strings.Join(out, "\n"), nil
}

func assetContentHash(ctx context.Context, db *sql.DB, assetPath string) (string, error) {
	var hash string
	if err := db.QueryRowContext(ctx, "SELECT sha256 FROM assets WHERE path=?", assetPath).Scan(&hash); err != nil {
		return "", err
	}
	return hash, nil
}
