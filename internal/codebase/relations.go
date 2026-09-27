package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"sort"
	"strconv"
	"strings"
	"unicode/utf16"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type ResearchOptions struct {
	Semantic            *luals.Runtime `json:"-"`
	EnvironmentSnapshot string         `json:"environmentSnapshot,omitempty"`
	Flow                bool           `json:"flow,omitempty"`
}
type RelationQuery struct {
	SymbolID  string `json:"symbolId,omitempty"`
	Symbol    string `json:"symbol,omitempty"`
	Direction string `json:"direction,omitempty"`
	Limit     int    `json:"limit"`
	Cursor    string `json:"cursor,omitempty"`
}
type ResearchCoverage struct {
	State        string   `json:"state"`
	Structural   string   `json:"structural"`
	Semantic     string   `json:"semantic"`
	Environment  string   `json:"environment"`
	Flow         string   `json:"flow,omitempty"`
	ScannedFacts int      `json:"scannedFacts"`
	Reasons      []string `json:"reasons,omitempty"`
}
type ResearchRelation struct {
	Source           string   `json:"source"`
	Target           string   `json:"target"`
	SourceID         string   `json:"sourceId,omitempty"`
	TargetID         string   `json:"targetId,omitempty"`
	TargetRepository string   `json:"targetRepository,omitempty"`
	TargetCommit     string   `json:"targetCommit,omitempty"`
	TargetPath       string   `json:"targetPath,omitempty"`
	TargetLine       int      `json:"targetLine,omitempty"`
	Kind             string   `json:"kind"`
	State            string   `json:"state"`
	Path             string   `json:"path"`
	Line             int      `json:"line"`
	EndLine          int      `json:"endLine,omitempty"`
	Basis            []string `json:"basis"`
	Reason           string   `json:"reason,omitempty"`
}
type RelationResult struct {
	Repository          string                `json:"repository"`
	Product             string                `json:"product"`
	Commit              string                `json:"commit"`
	Snapshot            string                `json:"snapshot"`
	Symbol              *SymbolMatch          `json:"symbol,omitempty"`
	Candidates          []SymbolMatch         `json:"candidates,omitempty"`
	Relations           []ResearchRelation    `json:"relations"`
	Coverage            ResearchCoverage      `json:"coverage"`
	EnvironmentManifest *environment.Manifest `json:"environmentManifest,omitempty"`
	SemanticRuntime     *luals.Identity       `json:"semanticRuntime,omitempty"`
	Truncated           bool                  `json:"truncated"`
	NextCursor          string                `json:"nextCursor,omitempty"`
	Complete            bool                  `json:"complete"`
}
type SourceRelations struct {
	Result  RelationResult      `json:"result"`
	Capture evidence.CaptureRef `json:"capture"`
}

func RelateSource(ctx context.Context, root, snapshot string, query RelationQuery, options ...ResearchOptions) (SourceRelations, error) {
	var reading SourceRelations
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
	reading.Result, err = b.Relate(ctx, snapshot, pin, query, opt)
	if err != nil {
		return reading, err
	}
	locator := query.SymbolID
	if locator == "" {
		locator = query.Symbol
	}
	reading.Capture, err = captureSourceJSON(ctx, root, reading.Result, reading.Result.Complete, reading.Result.Truncated, evidence.Provenance{Kind: "source-refs", Locator: locator, Snapshot: snapshot, SourceCommit: pin.ExactCommit})
	return reading, err
}

func researchLimit(limit int) (int, error) {
	if limit == 0 {
		return 25, nil
	}
	if limit < 1 || limit > 200 {
		return 0, errors.New("codebase.invalid_relation_limit")
	}
	return limit, nil
}
func researchCursor(pin selection.SourcePin, identity, cursor string) (int, string, error) {
	h := sha256.Sum256([]byte(pin.Repository + "\x00" + pin.Product + "\x00" + pin.ExactCommit + "\x00" + identity))
	key := hex.EncodeToString(h[:8])
	if cursor == "" {
		return 0, key, nil
	}
	found, number, ok := strings.Cut(cursor, ":")
	if !ok || found != key {
		return 0, "", ErrInvalidSearchCursor
	}
	offset, err := strconv.Atoi(number)
	if err != nil || offset < 0 || offset > 10000 {
		return 0, "", ErrInvalidSearchCursor
	}
	return offset, key, nil
}

func (b *Browser) Relate(ctx context.Context, snapshot string, pin selection.SourcePin, query RelationQuery, opt ResearchOptions) (RelationResult, error) {
	result := RelationResult{Repository: pin.Repository, Product: pin.Product, Commit: pin.ExactCommit, Snapshot: snapshot, Candidates: []SymbolMatch{}, Relations: []ResearchRelation{}, Coverage: ResearchCoverage{State: "partial", Structural: "complete", Semantic: "unavailable", Environment: "unspecified", Reasons: []string{}}}
	if (query.SymbolID == "") == (query.Symbol == "") {
		return result, ErrInvalidSymbol
	}
	direction := query.Direction
	if direction == "" {
		direction = "both"
	}
	if direction != "incoming" && direction != "outgoing" && direction != "both" {
		return result, errors.New("codebase.invalid_relation_direction")
	}
	limit, err := researchLimit(query.Limit)
	if err != nil {
		return result, err
	}
	env, envState, envErr := b.researchEnvironment(ctx, pin, opt.EnvironmentSnapshot)
	if envErr != nil {
		return result, envErr
	}
	result.Coverage.Environment = envState
	if envState == "ready" {
		manifest := env.Manifest
		result.EnvironmentManifest = &manifest
		if !env.Coverage.Complete {
			result.Coverage.Environment = "partial"
			result.Coverage.Reasons = append(result.Coverage.Reasons, "client API environment has extraction gaps")
		}
	}
	runtimeKey := "static"
	if opt.Semantic != nil {
		runtimeKey = opt.Semantic.Identity.Version + ":" + opt.Semantic.Identity.SHA256
		runtime := opt.Semantic.Identity
		result.SemanticRuntime = &runtime
	}
	envKey := envState
	if result.EnvironmentManifest != nil {
		envKey = env.Manifest.InputSHA256 + ":" + env.Manifest.DefinitionsSHA256
	}
	offset, cursorKey, err := researchCursor(pin, query.SymbolID+"\x00"+query.Symbol+"\x00"+direction+"\x00"+strconv.Itoa(limit)+"\x00"+runtimeKey+"\x00"+envKey+"\x00"+strconv.FormatBool(opt.Flow), query.Cursor)
	if err != nil {
		return result, err
	}
	cache, summary, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	var declarations []SymbolMatch
	err = cache.scan(ctx, func(r sourceRecord) error {
		result.Coverage.ScannedFacts++
		if r.Kind != "symbol" || r.Symbol == nil {
			return nil
		}
		s := *r.Symbol
		if s.Kind == "declaration" && (query.SymbolID != "" && s.ID == query.SymbolID || query.Symbol != "" && s.Name == query.Symbol) {
			declarations = append(declarations, s)
		}
		return nil
	})
	if err != nil {
		return result, err
	}
	if len(declarations) > 1 && query.SymbolID == "" {
		result.Candidates = declarations
		if len(result.Candidates) > limit {
			result.Candidates = result.Candidates[:limit]
			result.Truncated = true
		}
		result.Coverage.Reasons = append(result.Coverage.Reasons, "name has multiple declaration identities; choose a symbolId")
		result.Complete = false
		return result, nil
	}
	if query.SymbolID != "" && len(declarations) == 0 {
		return result, ErrInvalidSymbol
	}
	name := query.Symbol
	if len(declarations) == 1 {
		result.Symbol = &declarations[0]
		name = declarations[0].Name
	}
	var edges []SymbolMatch
	edgeCut := false
	err = cache.scan(ctx, func(r sourceRecord) error {
		if r.Kind != "symbol" || r.Symbol == nil || r.Symbol.Kind != "relationship" {
			return nil
		}
		s := *r.Symbol
		incoming := s.Target == name
		outgoing := s.Name == name
		if direction == "incoming" && !incoming || direction == "outgoing" && !outgoing || direction == "both" && !incoming && !outgoing {
			return nil
		}
		if len(edges) >= 10000 {
			edgeCut = true
			return nil
		}
		edges = append(edges, s)
		return nil
	})
	if err != nil {
		return result, err
	}
	for _, edge := range edges {
		incoming := edge.Target == name
		outgoing := edge.Name == name
		if direction == "incoming" && !incoming || direction == "outgoing" && !outgoing || direction == "both" && !incoming && !outgoing {
			continue
		}
		state := "static_candidate"
		reason := "name and structural source relation only; binding is not proven"
		if edge.Confidence == "dynamic-unresolved" {
			state = "unknown"
			reason = "dynamic source expression"
		}
		if edge.Category == "contains" || edge.Category == "event-name" || edge.Category == "inherits" || edge.Category == "event-registration" {
			state = "structural"
			reason = "literal or parser-derived WoW relation; runtime dispatch is not proven"
		}
		relation := ResearchRelation{Source: edge.Name, Target: edge.Target, Kind: edge.Category, State: state, Path: edge.Path, Line: edge.Line, Basis: []string{"static-extraction"}, Reason: reason}
		// Name agreement is not lexical binding. IDs are attached only when a
		// semantic resolution names an exact declaration location.
		result.Relations = append(result.Relations, relation)
	}
	if result.Symbol == nil && len(result.Relations) == 0 {
		result.Coverage.Reasons = append(result.Coverage.Reasons, "no declaration or structural relation in indexed coverage")
	}
	if opt.Semantic != nil && result.Symbol != nil && direction != "outgoing" {
		semantic, reason := b.cachedSemantic(ctx, pin, cache.manifest.RecordsHash, env.Manifest, opt.Semantic, *result.Symbol, "incoming", func() ([]ResearchRelation, string) {
			return b.semanticReferences(ctx, cache, pin, *result.Symbol, opt.Semantic, env.Definitions, env.Mappings)
		})
		if reason != "" {
			result.Coverage.Reasons = append(result.Coverage.Reasons, reason)
			result.Coverage.Semantic = "partial"
		} else {
			result.Coverage.Semantic = "complete"
		}
		result.Relations = append(result.Relations, semantic...)
	} else if opt.Semantic == nil {
		result.Coverage.Reasons = append(result.Coverage.Reasons, "verified LuaLS runtime unavailable or static-only requested")
	}
	if opt.Semantic != nil && result.Symbol != nil && direction != "incoming" {
		outgoing, reason := b.cachedSemantic(ctx, pin, cache.manifest.RecordsHash, env.Manifest, opt.Semantic, *result.Symbol, "outgoing", func() ([]ResearchRelation, string) {
			return b.semanticOutgoing(ctx, cache, pin, *result.Symbol, edges, opt.Semantic, env.Definitions, env.Mappings, env.Manifest)
		})
		if reason != "" {
			result.Coverage.Semantic = "partial"
			result.Coverage.Reasons = append(result.Coverage.Reasons, reason)
		} else if result.Coverage.Semantic != "partial" {
			result.Coverage.Semantic = "complete"
		}
		result.Relations = append(result.Relations, outgoing...)
	}
	if result.Coverage.Environment == "missing-client-pin" {
		result.Coverage.Reasons = append(result.Coverage.Reasons, "third-party source has no selected client API environment")
	}
	if !summary.Complete {
		result.Coverage.Structural = "partial"
		result.Coverage.Reasons = append(result.Coverage.Reasons, "some source files have parser diagnostics")
	}
	sortResearchRelations(result.Relations)
	if offset > len(result.Relations) {
		return result, ErrInvalidSearchCursor
	}
	all := len(result.Relations)
	end := min(all, offset+limit)
	result.Relations = result.Relations[offset:end]
	result.Truncated = end < all || edgeCut
	if edgeCut {
		result.Coverage.Structural = "partial"
		result.Coverage.Reasons = append(result.Coverage.Reasons, "matching structural relations exceeded 10000-entry budget")
	}
	if end < all {
		result.NextCursor = cursorKey + ":" + strconv.Itoa(end)
	}
	result.Complete = !result.Truncated && result.Coverage.Structural == "complete" && result.Coverage.Semantic == "complete" && result.Coverage.Environment == "ready"
	if result.Complete {
		result.Coverage.State = "complete"
	}
	return result, nil
}

func sortResearchRelations(rows []ResearchRelation) {
	// Keep structural and LSP evidence separate; stable ordering makes cursor
	// pages reproducible for this fixed snapshot.
	sort.Slice(rows, func(i, j int) bool {
		a, z := rows[i], rows[j]
		if a.Path != z.Path {
			return a.Path < z.Path
		}
		if a.Line != z.Line {
			return a.Line < z.Line
		}
		if a.Kind != z.Kind {
			return a.Kind < z.Kind
		}
		if a.Source != z.Source {
			return a.Source < z.Source
		}
		return a.Target < z.Target
	})
}

func (b *Browser) semanticReferences(ctx context.Context, cache *snapshotCache, pin selection.SourcePin, symbol SymbolMatch, runtime *luals.Runtime, definitions []byte, mappings []environment.DefinitionMapping) ([]ResearchRelation, string) {
	queryPath := symbol.Path
	var position luals.Position
	if strings.HasPrefix(symbol.Category, "api-") {
		matched := []environment.DefinitionMapping{}
		for _, mapping := range mappings {
			if symbol.Category == "api-function" && mapping.Kind == "function" && mapping.SourcePath == symbol.Path && mapping.Callable == symbol.Name && symbol.Line <= mapping.SourceLine && mapping.SourceLine <= max(symbol.EndLine, symbol.Line) {
				matched = append(matched, mapping)
			}
		}
		if len(matched) != 1 {
			return nil, "API source declaration has no unique generated definition mapping"
		}
		queryPath = luals.DefinitionPath
		position = luals.Position{Line: matched[0].GeneratedLine, Character: matched[0].GeneratedUTF16Column}
	} else if !strings.HasSuffix(strings.ToLower(symbol.Path), ".lua") {
		return nil, "LuaLS does not resolve this source type"
	}
	if queryPath == symbol.Path {
		var err error
		position, err = symbolPosition(ctx, cache, symbol)
		if err != nil {
			return nil, err.Error()
		}
	}
	lease, err := b.AcquireWorktree(ctx, pin)
	if err != nil {
		return nil, "fixed worktree unavailable: " + err.Error()
	}
	defer lease.Close()
	analysis, err := runtime.AnalyzeWorkspace(ctx, lease.Path(), definitions, []luals.Query{{Kind: luals.References, Path: queryPath, Position: position}, {Kind: luals.Hover, Path: queryPath, Position: position}})
	if err != nil {
		return nil, "LuaLS relation request failed: " + err.Error()
	}
	rows := []ResearchRelation{}
	for _, answer := range analysis.Results {
		if answer.Query.Kind != luals.References {
			continue
		}
		if answer.State != "complete" {
			return rows, "LuaLS references incomplete: " + answer.Reason
		}
		for _, location := range answer.Locations {
			if location.Path == luals.DefinitionPath {
				continue
			}
			line := location.Range.Start.Line + 1
			if location.Path == symbol.Path && line == symbol.Line {
				continue
			}
			rows = append(rows, ResearchRelation{Source: location.Path, Target: symbol.Name, TargetID: symbol.ID, Kind: "reference", State: "resolved", Path: location.Path, Line: line, EndLine: location.Range.End.Line + 1, Basis: []string{"LuaLS public textDocument/references", "fixed worktree " + pin.ExactCommit}})
		}
	}
	if analysis.State != "complete" {
		return rows, "LuaLS analysis partial"
	}
	return rows, ""
}

func (b *Browser) semanticOutgoing(ctx context.Context, cache *snapshotCache, pin selection.SourcePin, symbol SymbolMatch, edges []SymbolMatch, runtime *luals.Runtime, definitions []byte, mappings []environment.DefinitionMapping, manifest environment.Manifest) ([]ResearchRelation, string) {
	queries := []luals.Query{}
	sites := []SymbolMatch{}
	uncertain := false
	for _, edge := range edges {
		if edge.Name != symbol.Name || edge.Category != "call" || edge.Path != symbol.Path || edge.Line < symbol.Line || edge.Line > symbol.EndLine {
			continue
		}
		if len(queries) >= 128 {
			uncertain = true
			break
		}
		position, err := symbolPosition(ctx, cache, SymbolMatch{Name: edge.Target, Path: edge.Path, Line: edge.Line})
		if err != nil {
			uncertain = true
			continue
		}
		queries = append(queries, luals.Query{Kind: luals.Definition, Path: edge.Path, Position: position})
		sites = append(sites, edge)
	}
	if len(queries) == 0 {
		if uncertain {
			return nil, "outgoing call locations could not be uniquely positioned"
		}
		return nil, ""
	}
	lease, err := b.AcquireWorktree(ctx, pin)
	if err != nil {
		return nil, "fixed worktree unavailable: " + err.Error()
	}
	defer lease.Close()
	analysis, err := runtime.AnalyzeWorkspace(ctx, lease.Path(), definitions, queries)
	if err != nil {
		return nil, "LuaLS outgoing definitions failed: " + err.Error()
	}
	lookup := map[string][]SymbolMatch{}
	err = cache.scan(ctx, func(r sourceRecord) error {
		if r.Kind == "symbol" && r.Symbol != nil && r.Symbol.Kind == "declaration" {
			s := *r.Symbol
			key := s.Path + "\x00" + strconv.Itoa(s.Line)
			lookup[key] = append(lookup[key], s)
		}
		return nil
	})
	if err != nil {
		return nil, err.Error()
	}
	generated := map[int][]environment.DefinitionMapping{}
	for _, mapping := range mappings {
		generated[mapping.GeneratedLine] = append(generated[mapping.GeneratedLine], mapping)
	}
	rows := []ResearchRelation{}
	for i, answer := range analysis.Results {
		if i >= len(sites) {
			uncertain = true
			break
		}
		site := sites[i]
		if answer.State != "complete" || len(answer.Locations) != 1 {
			uncertain = true
			continue
		}
		location := answer.Locations[0]
		targetPath, targetLine := location.Path, location.Range.Start.Line+1
		if location.Path == luals.DefinitionPath {
			found := []environment.DefinitionMapping{}
			for _, candidate := range generated[location.Range.Start.Line] {
				if candidate.GeneratedUTF16Column == location.Range.Start.Character {
					found = append(found, candidate)
				}
			}
			if len(found) != 1 {
				uncertain = true
				continue
			}
			mapping := found[0]
			h := sha256.Sum256([]byte(manifest.Identity.Repository + "\x00" + manifest.Identity.Commit + "\x00" + mapping.Callable + "\x00" + mapping.SourcePath + "\x00" + strconv.Itoa(mapping.SourceLine)))
			rows = append(rows, ResearchRelation{Source: symbol.Name, SourceID: symbol.ID, Target: mapping.Callable, TargetID: "API-" + hex.EncodeToString(h[:16]), TargetRepository: manifest.Identity.Repository, TargetCommit: manifest.Identity.Commit, TargetPath: mapping.SourcePath, TargetLine: mapping.SourceLine, Kind: "call", State: "resolved", Path: site.Path, Line: site.Line, Basis: []string{"LuaLS public textDocument/definition", "generated declaration mapped to pinned API source"}})
			continue
		}
		candidates := lookup[targetPath+"\x00"+strconv.Itoa(targetLine)]
		if len(candidates) != 1 {
			uncertain = true
			continue
		}
		target := candidates[0]
		rows = append(rows, ResearchRelation{Source: symbol.Name, SourceID: symbol.ID, Target: target.Name, TargetID: target.ID, Kind: "call", State: "resolved", Path: site.Path, Line: site.Line, Basis: []string{"LuaLS public textDocument/definition", "fixed worktree " + pin.ExactCommit}})
	}
	if uncertain || analysis.State != "complete" {
		return rows, "some outgoing call definitions are ambiguous, unsupported, or beyond the 128-query budget"
	}
	return rows, ""
}

func symbolPosition(ctx context.Context, cache *snapshotCache, s SymbolMatch) (luals.Position, error) {
	data, _, err := cache.document(ctx, s.Path)
	if err != nil {
		return luals.Position{}, err
	}
	lines := strings.Split(string(data), "\n")
	if s.Line < 1 || s.Line > len(lines) {
		return luals.Position{}, errors.New("codebase.symbol_line_out_of_range")
	}
	line := lines[s.Line-1]
	needle := s.Name
	if dot := strings.LastIndexAny(needle, ".:"); dot >= 0 {
		needle = needle[dot+1:]
	}
	if needle == "" {
		return luals.Position{}, ErrInvalidSymbol
	}
	start := strings.Index(line, needle)
	if start < 0 || strings.Contains(line[start+len(needle):], needle) {
		return luals.Position{}, errors.New("codebase.symbol_position_ambiguous")
	}
	utf16Column := 0
	for _, r := range line[:start] {
		utf16Column += utf16.RuneLen(r)
	}
	return luals.Position{Line: s.Line - 1, Character: utf16Column}, nil
}
