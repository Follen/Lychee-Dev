package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"path"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/codebase/flow"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type ContextQuery struct {
	SymbolID string `json:"symbolId,omitempty"`
	Symbol   string `json:"symbol,omitempty"`
	Limit    int    `json:"limit"`
	MaxBytes int    `json:"maxBytes"`
	MaxLines int    `json:"maxLines"`
	Depth    int    `json:"depth"`
	Cursor   string `json:"cursor,omitempty"`
}
type ContextSnippet struct {
	Path        string `json:"path"`
	FirstLine   int    `json:"firstLine"`
	LastLine    int    `json:"lastLine"`
	ContentHash string `json:"contentHash"`
	Text        string `json:"text"`
	Basis       string `json:"basis"`
}
type ContextResult struct {
	Repository          string                 `json:"repository"`
	Product             string                 `json:"product"`
	Commit              string                 `json:"commit"`
	Snapshot            string                 `json:"snapshot"`
	Symbol              *SymbolMatch           `json:"symbol,omitempty"`
	APIFact             *environment.APIRecord `json:"apiFact,omitempty"`
	EnvironmentManifest *environment.Manifest  `json:"environmentManifest,omitempty"`
	SemanticRuntime     *luals.Identity        `json:"semanticRuntime,omitempty"`
	Candidates          []SymbolMatch          `json:"candidates,omitempty"`
	Snippets            []ContextSnippet       `json:"snippets"`
	Relations           []ResearchRelation     `json:"relations"`
	LoadEvidence        []ResearchRelation     `json:"loadEvidence"`
	Flow                *flow.Result           `json:"flow,omitempty"`
	Coverage            ResearchCoverage       `json:"coverage"`
	Truncated           bool                   `json:"truncated"`
	NextCursor          string                 `json:"nextCursor,omitempty"`
	Complete            bool                   `json:"complete"`
}
type SourceContext struct {
	Result  ContextResult       `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

func ContextSource(ctx context.Context, root, snapshot string, query ContextQuery, options ...ResearchOptions) (SourceContext, error) {
	var reading SourceContext
	pin, err := pinnedSource(ctx, root, snapshot)
	if err != nil {
		return reading, err
	}
	s, err := vault.OpenStore(root)
	if err != nil {
		return reading, err
	}
	b := OpenBrowser(s)
	if err := b.EnsureIndex(ctx, pin); err != nil {
		return reading, err
	}
	opt := ResearchOptions{}
	if len(options) > 0 {
		opt = options[0]
	}
	reading.Result, err = b.Context(ctx, snapshot, pin, query, opt)
	if err != nil {
		return reading, err
	}
	locator := query.SymbolID
	if locator == "" {
		locator = query.Symbol
	}
	reading.Capture, err = captureSourceJSON(ctx, root, reading.Result, reading.Result.Complete, reading.Result.Truncated, evidence.Provenance{Kind: "source-context", Locator: locator, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return reading, err
}

func (b *Browser) Context(ctx context.Context, snapshot string, pin selection.SourcePin, query ContextQuery, opt ResearchOptions) (ContextResult, error) {
	result := ContextResult{Repository: pin.Repository, Product: pin.Product, Commit: pin.ExactCommit, Snapshot: snapshot, Snippets: []ContextSnippet{}, Relations: []ResearchRelation{}, LoadEvidence: []ResearchRelation{}, Candidates: []SymbolMatch{}}
	if query.Depth < 0 || query.Depth > 4 {
		return result, errors.New("codebase.invalid_context_depth")
	}
	if query.MaxBytes == 0 {
		query.MaxBytes = 16 << 10
	}
	if query.MaxBytes < 1 || query.MaxBytes > 1<<20 {
		return result, errors.New("codebase.invalid_context_bytes")
	}
	if query.MaxLines == 0 {
		query.MaxLines = 80
	}
	if query.MaxLines < 1 || query.MaxLines > 2000 {
		return result, errors.New("codebase.invalid_context_lines")
	}
	limit, err := researchLimit(query.Limit)
	if err != nil {
		return result, err
	}
	mode := "static"
	if opt.Semantic != nil {
		mode = opt.Semantic.Identity.Version + ":" + opt.Semantic.Identity.SHA256
	}
	identity := strings.Join([]string{pin.Repository, pin.Product, pin.ExactCommit, query.SymbolID, query.Symbol, strconv.Itoa(limit), strconv.Itoa(query.MaxBytes), strconv.Itoa(query.MaxLines), strconv.Itoa(query.Depth), opt.EnvironmentSnapshot, strconv.FormatBool(opt.Flow), mode}, "\x00")
	identityHash := sha256.Sum256([]byte(identity))
	cursorKey := hex.EncodeToString(identityHash[:8])
	relationCursor, siteOffset, lineOffset, byteOffset, err := decodeContextCursor(query.Cursor, cursorKey)
	if err != nil {
		return result, err
	}
	related, err := b.Relate(ctx, snapshot, pin, RelationQuery{SymbolID: query.SymbolID, Symbol: query.Symbol, Direction: "both", Limit: limit, Cursor: relationCursor}, opt)
	if err != nil {
		return result, err
	}
	result.Symbol = related.Symbol
	result.EnvironmentManifest = related.EnvironmentManifest
	result.SemanticRuntime = related.SemanticRuntime
	result.Candidates = related.Candidates
	if query.Depth > 0 && siteOffset == 0 {
		result.Relations = related.Relations
	}
	result.Coverage = related.Coverage
	result.Truncated = related.Truncated
	if len(result.Candidates) > 0 {
		return result, nil
	}
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	type site struct {
		path        string
		first, last int
		basis       string
	}
	sites := []site{}
	if result.Symbol != nil {
		sites = append(sites, site{result.Symbol.Path, result.Symbol.Line, max(result.Symbol.EndLine, result.Symbol.Line), "definition"})
		if strings.HasPrefix(result.Symbol.Category, "api-") && result.EnvironmentManifest != nil {
			parsed, state, err := b.researchEnvironment(ctx, pin, opt.EnvironmentSnapshot)
			if err != nil {
				return result, err
			}
			if state == "ready" {
				for _, fact := range parsed.Facts {
					if fact.Name == result.Symbol.Name && fact.Path == result.Symbol.Path && result.Symbol.Line <= fact.Line && fact.Line <= max(result.Symbol.EndLine, result.Symbol.Line) {
						if result.APIFact != nil {
							result.APIFact = nil
							result.Coverage.Reasons = append(result.Coverage.Reasons, "API fact identity ambiguous")
							break
						}
						copy := fact
						result.APIFact = &copy
					}
				}
			}
		}
	}
	if query.Depth > 0 {
		for _, relation := range related.Relations {
			if relation.Path != "" && relation.Line > 0 {
				sites = append(sites, site{relation.Path, max(relation.Line-2, 1), relation.Line + 2, "relation:" + relation.Kind})
			}
		}
		calleeNames := map[string]bool{}
		for _, relation := range related.Relations {
			if relation.Kind == "call" && relation.Target != "" && relation.Target != "<dynamic>" {
				calleeNames[relation.Target] = true
			}
		}
		if len(calleeNames) > 0 {
			candidates := map[string][]SymbolMatch{}
			err = cache.scan(ctx, func(r sourceRecord) error {
				if r.Kind != "symbol" || r.Symbol == nil || r.Symbol.Kind != "declaration" || !calleeNames[r.Symbol.Name] {
					return nil
				}
				if r.Symbol.Category == "function" || r.Symbol.Category == "local-function" {
					candidates[r.Symbol.Name] = append(candidates[r.Symbol.Name], *r.Symbol)
				}
				return nil
			})
			if err != nil {
				return result, err
			}
			names := make([]string, 0, len(candidates))
			for name := range candidates {
				names = append(names, name)
			}
			sort.Strings(names)
			for _, name := range names {
				declarations := candidates[name]
				if len(declarations) != 1 {
					result.Coverage.Reasons = append(result.Coverage.Reasons, "callee candidate ambiguous: "+name)
					continue
				}
				declaration := declarations[0]
				sites = append(sites, site{declaration.Path, declaration.Line, max(declaration.EndLine, declaration.Line), "candidate-callee:" + name})
			}
		}
	}
	loadTarget := ""
	if result.Symbol != nil {
		loadTarget = result.Symbol.Path
	}
	if loadTarget != "" {
		err = cache.scan(ctx, func(r sourceRecord) error {
			if r.Kind != "symbol" || r.Symbol == nil || r.Symbol.Kind != "load" {
				return nil
			}
			s := r.Symbol
			if loadedPath(s.Path, s.Target) == loadTarget {
				if len(result.LoadEvidence) < limit {
					result.LoadEvidence = append(result.LoadEvidence, ResearchRelation{Source: s.Name, Target: s.Target, Kind: "load", State: "structural", Path: s.Path, Line: s.Line, Basis: []string{"TOC/XML literal load"}, Reason: "load declaration; runtime execution not proven"})
				} else {
					result.Truncated = true
				}
			}
			return nil
		})
		if err != nil {
			return result, err
		}
	}
	if query.Depth > 1 {
		frontier := map[string]bool{}
		for _, relation := range related.Relations {
			frontier[relation.Source], frontier[relation.Target] = true, true
		}
		visited := map[string]bool{}
		for depth := 2; depth <= query.Depth && len(frontier) > 0; depth++ {
			next := map[string]bool{}
			err = cache.scan(ctx, func(r sourceRecord) error {
				if r.Kind != "symbol" || r.Symbol == nil || r.Symbol.Kind != "relationship" {
					return nil
				}
				s := r.Symbol
				if !frontier[s.Name] && !frontier[s.Target] {
					return nil
				}
				if len(sites) >= 2000 {
					result.Truncated = true
					return nil
				}
				k := s.Path + "\x00" + strconv.Itoa(s.Line) + "\x00" + s.Category
				if visited[k] {
					return nil
				}
				visited[k] = true
				sites = append(sites, site{s.Path, max(s.Line-2, 1), s.Line + 2, "static-depth:" + strconv.Itoa(depth)})
				next[s.Name], next[s.Target] = true, true
				return nil
			})
			if err != nil {
				return result, err
			}
			frontier = next
		}
	}
	seen := map[string]bool{}
	usedBytes, usedLines := 0, 0
	unique := make([]site, 0, len(sites))
	for _, site := range sites {
		if !sourcePath(site.path) {
			continue
		}
		key := site.path + "\x00" + strconv.Itoa(site.first) + "\x00" + strconv.Itoa(site.last)
		if seen[key] {
			continue
		}
		seen[key] = true
		unique = append(unique, site)
	}
	if siteOffset > len(unique) {
		return result, ErrInvalidSearchCursor
	}
	for i := siteOffset; i < len(unique); i++ {
		site := unique[i]
		if len(result.Snippets) >= limit {
			result.Truncated = true
			result.NextCursor = encodeContextCursor(cursorKey, relationCursor, i, lineOffset, byteOffset)
			break
		}
		data, digest, err := cache.document(ctx, site.path)
		if err != nil {
			result.Coverage.Reasons = append(result.Coverage.Reasons, "snippet unavailable: "+site.path+": "+err.Error())
			result.Coverage.State = "partial"
			continue
		}
		lineBudget := query.MaxLines - usedLines
		if lineBudget <= 0 {
			result.Truncated = true
			result.NextCursor = encodeContextCursor(cursorKey, relationCursor, i, lineOffset, byteOffset)
			break
		}
		start := site.first
		if i == siteOffset && lineOffset > 0 {
			start = lineOffset
		}
		startByte := 0
		if i == siteOffset {
			startByte = byteOffset
		}
		text, last, nextLine, nextByte, done, excerptErr := contextExcerpt(data, start, site.last, startByte, query.MaxBytes-usedBytes, lineBudget)
		if excerptErr != nil {
			return result, excerptErr
		}
		if text == "" {
			continue
		}
		result.Snippets = append(result.Snippets, ContextSnippet{Path: site.path, FirstLine: start, LastLine: last, ContentHash: digest, Text: text, Basis: site.basis})
		usedBytes += len(text)
		usedLines += last - start + 1
		if !done {
			result.Truncated = true
			result.NextCursor = encodeContextCursor(cursorKey, relationCursor, i, nextLine, nextByte)
			break
		}
		if usedBytes >= query.MaxBytes || usedLines >= query.MaxLines {
			if i+1 < len(unique) {
				result.Truncated = true
				result.NextCursor = encodeContextCursor(cursorKey, relationCursor, i+1, 0, 0)
			}
			break
		}
	}
	if result.NextCursor == "" && related.NextCursor != "" {
		result.NextCursor = encodeContextCursor(cursorKey, related.NextCursor, 0, 0, 0)
	}
	if opt.Flow {
		if result.Symbol == nil || !strings.HasSuffix(strings.ToLower(result.Symbol.Path), ".lua") {
			result.Coverage.Flow = "unsupported"
			result.Coverage.Reasons = append(result.Coverage.Reasons, "value propagation requires a selected Lua declaration")
		} else {
			parsed, state, envErr := b.researchEnvironment(ctx, pin, opt.EnvironmentSnapshot)
			if envErr != nil {
				return result, envErr
			}
			if state != "ready" {
				result.Coverage.Flow = "unavailable"
				result.Coverage.Reasons = append(result.Coverage.Reasons, "versioned client API rule set unavailable")
			} else {
				rules, ruleErr := flow.RulesFromFacts(flow.RuleIdentity{Client: parsed.Manifest.Identity.Client, APICommit: parsed.Manifest.Identity.Commit, MetadataDigest: parsed.Manifest.InputSHA256, Version: "secret-flow-v1"}, parsed)
				if ruleErr != nil {
					return result, ruleErr
				}
				files := map[string][]byte{}
				flowPaths := []string{result.Symbol.Path}
				for _, site := range unique {
					if site.path != result.Symbol.Path {
						flowPaths = append(flowPaths, site.path)
					}
				}
				flowBytes := 0
				flowOmitted := false
				for _, name := range flowPaths {
					if _, ok := files[name]; ok || !strings.HasSuffix(strings.ToLower(name), ".lua") {
						continue
					}
					if len(files) >= 32 {
						flowOmitted = true
						result.Coverage.Reasons = append(result.Coverage.Reasons, "flow closure exceeded 32-file budget")
						break
					}
					data, _, readErr := cache.document(ctx, name)
					if readErr != nil || len(data) > 1<<20 || flowBytes+len(data) > 32<<20 {
						flowOmitted = true
						result.Coverage.Reasons = append(result.Coverage.Reasons, "flow closure omitted a file: "+name)
						continue
					}
					files[name] = data
					flowBytes += len(data)
				}
				analysis, flowErr := flow.Analyze(ctx, flow.Request{Files: files, Target: flow.Target{Path: result.Symbol.Path, Line: result.Symbol.Line, Symbol: result.Symbol.Name}, Rules: rules, Budget: flow.Budget{MaxFiles: 32, MaxDepth: max(1, query.Depth), MaxOutputBytes: max(2048, min(query.MaxBytes, 64<<10))}})
				if flowErr != nil {
					return result, flowErr
				}
				result.Flow = &analysis
				result.Coverage.Flow = analysis.Coverage.State
				if analysis.Truncated || flowOmitted {
					result.Truncated = result.Truncated || analysis.Truncated
					result.Coverage.Flow = "partial"
				}
			}
		}
	}
	result.Complete = related.Complete && !result.Truncated && result.Coverage.State != "partial" && (!opt.Flow || result.Coverage.Flow == "bounded_complete")
	if result.Truncated {
		result.Coverage.State = "partial"
	}
	return result, nil
}

func loadedPath(source, target string) string {
	target = strings.ReplaceAll(target, "\\", "/")
	if target == "" || strings.HasPrefix(target, "/") || strings.Contains(target, ":") {
		return ""
	}
	resolved := path.Clean(path.Join(path.Dir(source), target))
	if resolved == ".." || strings.HasPrefix(resolved, "../") || resolved == "." {
		return ""
	}
	return resolved
}

func encodeContextCursor(key, relationCursor string, site, line, byteOffset int) string {
	return key + ":" + base64.RawURLEncoding.EncodeToString([]byte(relationCursor)) + ":" + strconv.Itoa(site) + ":" + strconv.Itoa(line) + ":" + strconv.Itoa(byteOffset)
}

func decodeContextCursor(cursor, key string) (string, int, int, int, error) {
	if cursor == "" {
		return "", 0, 0, 0, nil
	}
	parts := strings.Split(cursor, ":")
	if len(parts) != 5 || parts[0] != key {
		return "", 0, 0, 0, ErrInvalidSearchCursor
	}
	raw, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || len(raw) > 128 {
		return "", 0, 0, 0, ErrInvalidSearchCursor
	}
	site, err := strconv.Atoi(parts[2])
	if err != nil || site < 0 || site > 2000 {
		return "", 0, 0, 0, ErrInvalidSearchCursor
	}
	line, err := strconv.Atoi(parts[3])
	if err != nil || line < 0 || line > 1000000 {
		return "", 0, 0, 0, ErrInvalidSearchCursor
	}
	byteOffset, err := strconv.Atoi(parts[4])
	if err != nil || byteOffset < 0 || byteOffset > 1<<20 {
		return "", 0, 0, 0, ErrInvalidSearchCursor
	}
	return string(raw), site, line, byteOffset, nil
}

func contextExcerpt(data []byte, first, last, startByte, maxBytes, maxLines int) (string, int, int, int, bool, error) {
	lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
	if first > last || first > len(lines) {
		return "", first - 1, first, 0, true, nil
	}
	var out strings.Builder
	lastOutput := first - 1
	line := first
	byteAt := startByte
	for line <= last && line <= len(lines) && line-first < maxLines {
		whole := fmt.Sprintf("%d: %s", line, strings.TrimSuffix(lines[line-1], "\r"))
		if byteAt > len(whole) {
			return "", 0, 0, 0, false, ErrInvalidSearchCursor
		}
		separator := 0
		if out.Len() > 0 {
			separator = 1
		}
		available := maxBytes - out.Len() - separator
		if available <= 0 {
			break
		}
		part := whole[byteAt:]
		if len(part) > available {
			cut := available
			for cut > 0 && !utf8.ValidString(part[:cut]) {
				cut--
			}
			if cut == 0 {
				return "", 0, 0, 0, false, ErrSourceBudget
			}
			if separator > 0 {
				out.WriteByte('\n')
			}
			out.WriteString(part[:cut])
			return out.String(), line, line, byteAt + cut, false, nil
		}
		if separator > 0 {
			out.WriteByte('\n')
		}
		out.WriteString(part)
		lastOutput = line
		line++
		byteAt = 0
	}
	return out.String(), lastOutput, line, 0, line > last || line > len(lines), nil
}
