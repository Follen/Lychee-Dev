package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"sort"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

type SearchMode string

const (
	SearchModePrecise     SearchMode = "precise"
	SearchModeExploratory SearchMode = "exploratory"
)

type SearchQuery struct {
	Mode   SearchMode `json:"mode"`
	Text   string     `json:"text"`
	Topic  string     `json:"topic"`
	Limit  int        `json:"limit"`
	Cursor string     `json:"cursor,omitempty"`
}
type Match struct {
	SymbolID    string         `json:"symbolId,omitempty"`
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
	SourceID       string         `json:"sourceId"`
	Product        string         `json:"product"`
	RequestedRef   string         `json:"requestedRef,omitempty"`
	MatchedTag     any            `json:"matchedTag"`
	ResolvedCommit string         `json:"resolvedCommit"`
	SnapshotID     string         `json:"snapshotId"`
	Results        []Match        `json:"results"`
	Relations      []Relation     `json:"relations,omitempty"`
	Suggestions    []string       `json:"suggestions,omitempty"`
	Complete       bool           `json:"complete"`
	Truncated      bool           `json:"truncated"`
	NextCursor     string         `json:"nextCursor,omitempty"`
	Coverage       SourceCoverage `json:"coverage"`
}
type searchCandidate struct {
	symbol  SymbolMatch
	matched string
	rank    int
}

func (b *Browser) Search(ctx context.Context, snapshotID string, pin selection.SourcePin, query SearchQuery) (SearchResponse, error) {
	response := SearchResponse{SourceID: pin.Repository, Product: pin.Product, RequestedRef: pin.RequestedRef, MatchedTag: nil, ResolvedCommit: pin.ExactCommit, SnapshotID: snapshotID, Results: []Match{}}
	text := strings.TrimSpace(query.Text)
	if text == "" || len(text) > 512 {
		return response, errors.New("codebase.query_required")
	}
	topic := strings.ToLower(strings.TrimSpace(query.Topic))
	if err := searchTopicFilter(topic); err != nil {
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
	identity := sha256.Sum256([]byte(pin.Repository + "\x00" + pin.Product + "\x00" + pin.ExactCommit + "\x00" + string(query.Mode) + "\x00" + text + "\x00" + topic))
	cursorKey := hex.EncodeToString(identity[:8])
	offset := 0
	if query.Cursor != "" {
		key, number, ok := strings.Cut(query.Cursor, ":")
		if !ok || key != cursorKey {
			return response, ErrInvalidSearchCursor
		}
		var err error
		offset, err = strconv.Atoi(number)
		if err != nil || offset < 0 || offset > 10000 {
			return response, ErrInvalidSearchCursor
		}
	}
	cache, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return response, err
	}
	response.Coverage = summary.Coverage()
	broad := query.Mode == SearchModeExploratory
	var exact, prefix, full, assets []searchCandidate
	candidateCut := false
	var relations []Relation
	documents := map[string]sourceRecord{}
	assetRows := map[string]sourceRecord{}
	normalized := normalizeAssetPath(text)
	tokens := strings.Fields(strings.ToLower(text))
	err = cache.scan(ctx, func(r sourceRecord) error {
		if r.Kind == "document" {
			documents[r.Path] = r
			return nil
		}
		if r.Kind == "asset" {
			assetRows[r.Path] = r
		}
		if r.Kind == "asset" && topic == "asset" && r.Asset != nil && strings.Contains(normalizeAssetPath(r.Path), normalized) {
			rank := 70
			if normalizeAssetPath(r.Path) == normalized {
				rank = 100
			}
			if len(assets) < 10000 {
				assets = append(assets, searchCandidate{symbol: SymbolMatch{Kind: "asset", Name: r.Path, Category: "asset", Confidence: "exact", Path: r.Path, Line: 1}, matched: "asset_path", rank: rank})
			} else {
				candidateCut = true
			}
			return nil
		}
		if r.Kind != "symbol" || r.Symbol == nil || topic == "asset" {
			return nil
		}
		s := *r.Symbol
		if s.Kind == "relationship" {
			match := s.Name == text || s.Target == text
			if broad {
				match = strings.Contains(strings.ToLower(s.Name), strings.ToLower(text)) || strings.Contains(strings.ToLower(s.Target), strings.ToLower(text))
			}
			if match && len(relations) <= limit {
				source := s.Name
				if source == "<file>" {
					source = s.Path
				}
				relations = append(relations, Relation{Source: source, Target: s.Target, Kind: s.Category, Confidence: s.Confidence, Path: s.Path, Line: s.Line})
			}
			return nil
		}
		if s.Kind != "declaration" && s.Kind != "header" {
			return nil
		}
		if topic == "api" && !strings.HasPrefix(s.Category, "api-") {
			return nil
		}
		if topic == "lua" || topic == "xml" || topic == "toc" {
			if !strings.HasSuffix(strings.ToLower(s.Path), "."+topic) {
				return nil
			}
		}
		lower := strings.ToLower(s.Name + " " + s.Target + " " + s.Signature)
		add := func(slice *[]searchCandidate, matched string, rank int) {
			if len(*slice) < 10000 {
				*slice = append(*slice, searchCandidate{symbol: s, matched: matched, rank: rank})
			} else {
				candidateCut = true
			}
		}
		if s.Kind == "declaration" && (s.Name == text || s.Target == text || strings.HasSuffix(s.Name, "."+text)) {
			add(&exact, "exact_symbol", 100)
		} else if s.Kind == "header" && s.Name == text {
			add(&exact, "exact_fact", 90)
		}
		if s.Kind == "declaration" && (strings.HasPrefix(s.Name, text) || strings.Contains(s.Name, "."+text)) {
			add(&prefix, "symbol_prefix", 80)
		} else if s.Kind == "header" && strings.HasPrefix(s.Name, text) {
			add(&prefix, "name_prefix", 80)
		}
		count := 0
		for _, token := range tokens {
			if strings.Contains(lower, token) {
				count++
			}
		}
		if len(tokens) > 0 && (broad && count > 0 || !broad && count == len(tokens)) {
			add(&full, "indexed_text", 70+min(9, count))
		}
		return nil
	})
	if err != nil {
		return response, err
	}
	cache.documents = documents
	cache.assets = assetRows
	cache.pathsOnce.Do(func() {})
	var candidates []searchCandidate
	if topic == "asset" {
		candidates = assets
	} else {
		candidates = exact
		if broad || len(candidates) == 0 {
			candidates = append(candidates, prefix...)
		}
		if broad || len(candidates) == 0 {
			candidates = append(candidates, full...)
		}
		if broad || len(candidates) == 0 {
			body, cut, err := cache.bodyCandidates(ctx, text, topic, 10000)
			if err != nil {
				return response, err
			}
			candidates = append(candidates, body...)
			candidateCut = candidateCut || cut
		}
	}
	sort.SliceStable(candidates, func(i, j int) bool {
		a, z := candidates[i], candidates[j]
		left := a.rank - roleRankPenalty(pathRole(a.symbol.Path))
		right := z.rank - roleRankPenalty(pathRole(z.symbol.Path))
		if left != right {
			return left > right
		}
		if a.symbol.Path != z.symbol.Path {
			return a.symbol.Path < z.symbol.Path
		}
		if a.symbol.Line != z.symbol.Line {
			return a.symbol.Line < z.symbol.Line
		}
		if a.symbol.Kind != z.symbol.Kind {
			return a.symbol.Kind < z.symbol.Kind
		}
		return a.symbol.Name < z.symbol.Name
	})
	seen := map[string]bool{}
	distinct := 0
	resultCut := false
	for _, c := range candidates {
		s := c.symbol
		key := s.ID
		if key == "" {
			key = fmt.Sprintf("%s:%d\x00%s\x00%s", s.Path, s.Line, s.Name, s.Kind)
		}
		if seen[key] {
			continue
		}
		seen[key] = true
		if distinct < offset {
			distinct++
			continue
		}
		if len(response.Results) >= limit {
			response.Truncated = true
			resultCut = true
			break
		}
		distinct++
		role := pathRole(s.Path)
		penalty := roleRankPenalty(role)
		var hash, snippet string
		if s.Kind == "asset" {
			if err := cache.loadPaths(ctx); err != nil {
				return response, err
			}
			if r, ok := cache.assets[s.Path]; ok && r.Asset != nil {
				hash = r.Asset.ContentHash
			}
			snippet = s.Path
		} else {
			data, digest, err := cache.document(ctx, s.Path)
			if err != nil {
				return response, err
			}
			hash = digest
			start, end := max(s.Line-3, 1), s.Line+3
			if c.matched == "exact_symbol" && s.EndLine >= s.Line {
				start, end = s.Line, s.EndLine
			}
			snippet = numberedLines(data, start, end, 80)
		}
		kind := s.Category
		if s.Kind == "header" {
			kind = "toc"
		}
		response.Results = append(response.Results, Match{SymbolID: s.ID, Kind: kind, Name: s.Name, Path: s.Path, Line: s.Line, MatchedBy: c.matched, Role: role, Confidence: s.Confidence, Score: c.rank - penalty, ScoreParts: map[string]int{"match": c.rank, "rolePenalty": -penalty}, ContentHash: hash, Excerpt: snippet})
	}
	sort.Slice(relations, func(i, j int) bool {
		a, z := relations[i], relations[j]
		if a.Confidence != z.Confidence {
			return confidenceRank(a.Confidence) < confidenceRank(z.Confidence)
		}
		if a.Path != z.Path {
			return a.Path < z.Path
		}
		return a.Line < z.Line
	})
	if offset == 0 {
		if len(relations) > limit {
			response.Truncated = true
			relations = relations[:limit]
		}
		response.Relations = relations
	}
	if resultCut && distinct <= 10000 {
		response.NextCursor = cursorKey + ":" + strconv.Itoa(distinct)
	}
	response.Truncated = response.Truncated || candidateCut
	response.Complete = summary.Complete && !response.Truncated
	if len(response.Results) == 0 {
		response.Suggestions = []string{"use explore for broader text and symbol matches", "check the topic, product and ref"}
	}
	return response, nil
}

func confidenceRank(v string) int {
	switch v {
	case "exact", "resolved":
		return 0
	case "inferred":
		return 1
	default:
		return 2
	}
}
func numberedLines(data []byte, start, end, maxLines int) string {
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
	return strings.Join(out, "\n")
}
func searchTopicFilter(topic string) error {
	switch topic {
	case "":
		return nil
	case "api":
		return nil
	case "lua", "xml", "toc", "asset":
		return nil
	default:
		return errors.New("codebase.invalid_topic: use api, lua, xml, toc, or asset")
	}
}
