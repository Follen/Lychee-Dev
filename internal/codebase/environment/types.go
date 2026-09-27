// Package environment extracts fixed Blizzard API documentation literals and
// emits a LuaLS definition file. It never executes source Lua or reads a client.
package environment

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"
)

const GeneratorVersion = "environment-v1"

var ErrBudget = errors.New("environment.input_budget")
var ErrIdentity = errors.New("environment.invalid_identity")

type Identity struct {
	Repository        string            `json:"repository"`
	Commit            string            `json:"commit"`
	Client            string            `json:"client"`
	DependencyCommits map[string]string `json:"dependencyCommits,omitempty"`
	GeneratorVersion  string            `json:"generatorVersion"`
}
type Limits struct {
	MaxDocuments       int
	MaxFileBytes       int
	MaxTotalBytes      int
	MaxRecords         int
	MaxDefinitionBytes int
}

func (l Limits) normalized() Limits {
	if l.MaxDocuments <= 0 {
		l.MaxDocuments = 4096
	}
	if l.MaxFileBytes <= 0 {
		l.MaxFileBytes = 4 << 20
	}
	if l.MaxTotalBytes <= 0 {
		l.MaxTotalBytes = 64 << 20
	}
	if l.MaxRecords <= 0 {
		l.MaxRecords = 100000
	}
	if l.MaxDefinitionBytes <= 0 {
		l.MaxDefinitionBytes = 8 << 20
	}
	return l
}

type Span struct {
	Path    string `json:"path"`
	Line    int    `json:"line"`
	EndLine int    `json:"endLine"`
}
type Slot struct {
	Index     int            `json:"index"` // exact 1-based API slot
	Name      string         `json:"name"`
	Type      string         `json:"type"`
	InnerType string         `json:"innerType,omitempty"`
	Nilable   bool           `json:"nilable"`
	Line      int            `json:"line"`
	Raw       map[string]any `json:"raw,omitempty"`
}
type APIRecord struct {
	Kind         string         `json:"kind"` // function, event, structure, enumeration, callback, script-object, or unknown
	Name         string         `json:"name"` // actual callable/type/event identity, never System display group fallback
	DisplayGroup string         `json:"displayGroup,omitempty"`
	Namespace    string         `json:"namespace,omitempty"`
	Path         string         `json:"path"`
	Line         int            `json:"line"`
	EndLine      int            `json:"endLine"`
	Parameters   []Slot         `json:"parameters,omitempty"`
	Returns      []Slot         `json:"returns,omitempty"`
	Payload      []Slot         `json:"payload,omitempty"`
	Fields       []Slot         `json:"fields,omitempty"`
	Values       []Slot         `json:"values,omitempty"`
	Raw          map[string]any `json:"raw,omitempty"`
}
type Gap struct {
	Path   string `json:"path"`
	Line   int    `json:"line,omitempty"`
	Code   string `json:"code"`
	Detail string `json:"detail,omitempty"`
}
type Coverage struct {
	Documents       int      `json:"documents"`
	ParsedDocuments int      `json:"parsedDocuments"`
	Records         int      `json:"records"`
	Emitted         int      `json:"emitted"`
	UnknownTypes    []string `json:"unknownTypes,omitempty"`
	Gaps            []Gap    `json:"gaps,omitempty"`
	Truncated       bool     `json:"truncated"`
	Complete        bool     `json:"complete"`
}
type Manifest struct {
	Schema            string   `json:"schema"`
	Identity          Identity `json:"identity"`
	InputSHA256       string   `json:"inputSHA256"`
	DefinitionsSHA256 string   `json:"definitionsSHA256"`
	FactsSHA256       string   `json:"factsSHA256"`
	Documents         int      `json:"documents"`
}
type DefinitionMapping struct {
	Callable             string `json:"callable"`
	SourcePath           string `json:"sourcePath"`
	SourceLine           int    `json:"sourceLine"`           // one-based
	GeneratedLine        int    `json:"generatedLine"`        // zero-based for LSP
	GeneratedUTF16Column int    `json:"generatedUTF16Column"` // zero-based for LSP
	Kind                 string `json:"kind"`
}
type Result struct {
	Manifest    Manifest            `json:"manifest"`
	Definitions []byte              `json:"definitions"`
	Facts       []APIRecord         `json:"facts"`
	Mappings    []DefinitionMapping `json:"mappings"`
	Coverage    Coverage            `json:"coverage"`
}

func Build(ctx context.Context, identity Identity, documents map[string][]byte, limits Limits) (Result, error) {
	result := Result{Facts: []APIRecord{}, Mappings: []DefinitionMapping{}, Coverage: Coverage{Gaps: []Gap{}, UnknownTypes: []string{}}}
	limits = limits.normalized()
	if identity.Repository == "" || identity.Commit == "" || identity.Client == "" || identity.GeneratorVersion == "" {
		return result, ErrIdentity
	}
	if len(documents) > limits.MaxDocuments {
		return result, ErrBudget
	}
	paths := make([]string, 0, len(documents))
	for path := range documents {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	h := sha256.New()
	total := 0
	for _, path := range paths {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		data := documents[path]
		if !filepath.IsLocal(filepath.FromSlash(path)) || strings.Contains(path, "\\") || !strings.HasSuffix(strings.ToLower(path), ".lua") || !utf8.Valid(data) || len(data) > limits.MaxFileBytes {
			return result, fmt.Errorf("%w: %s", ErrBudget, path)
		}
		total += len(data)
		if total > limits.MaxTotalBytes {
			return result, ErrBudget
		}
		fmt.Fprintf(h, "%d:%s:%d:", len(path), path, len(data))
		_, _ = h.Write(data)
		result.Coverage.Documents++
		records, gaps := parseDocument(ctx, path, data)
		if err := ctx.Err(); err != nil {
			return result, err
		}
		if len(gaps) == 0 || gaps[0].Code != "parse" {
			result.Coverage.ParsedDocuments++
		}
		result.Facts = append(result.Facts, records...)
		result.Coverage.Gaps = append(result.Coverage.Gaps, gaps...)
		if len(result.Facts) > limits.MaxRecords || len(result.Coverage.Gaps) > 10000 {
			return Result{}, ErrBudget
		}
	}
	result.Coverage.Records = len(result.Facts)
	sort.Slice(result.Facts, func(i, j int) bool {
		a, b := result.Facts[i], result.Facts[j]
		if a.Path != b.Path {
			return a.Path < b.Path
		}
		if a.Line != b.Line {
			return a.Line < b.Line
		}
		if a.Kind != b.Kind {
			return a.Kind < b.Kind
		}
		return a.Name < b.Name
	})
	definitions, mappings, unknown, gaps := emitDefinitions(result.Facts, limits.MaxDefinitionBytes)
	result.Definitions, result.Mappings = definitions, mappings
	result.Coverage.UnknownTypes = unknown
	result.Coverage.Gaps = append(result.Coverage.Gaps, gaps...)
	result.Coverage.Emitted = len(mappings)
	result.Coverage.Complete = len(result.Coverage.Gaps) == 0 && !result.Coverage.Truncated
	if len(definitions) > limits.MaxDefinitionBytes {
		return Result{}, ErrBudget
	}
	factsJSON, err := json.Marshal(result.Facts)
	if err != nil {
		return Result{}, err
	}
	defHash := sha256.Sum256(definitions)
	factsHash := sha256.Sum256(factsJSON)
	result.Manifest = Manifest{Schema: "lycheedev.source-environment.v1", Identity: identity, InputSHA256: hex.EncodeToString(h.Sum(nil)), DefinitionsSHA256: hex.EncodeToString(defHash[:]), FactsSHA256: hex.EncodeToString(factsHash[:]), Documents: result.Coverage.Documents}
	return result, nil
}
