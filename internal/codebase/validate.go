package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

// LoadSourceRef traces one duplicate-preserving load edge of the closure.
type LoadSourceRef struct {
	File string `json:"file"`
	Line int    `json:"line"`
	Kind string `json:"kind"`
}

// LoadFileRef is one document of the ordered TOC load closure. The TOC itself
// is reported through TOC and never listed as a loaded file.
type LoadFileRef struct {
	Path      string          `json:"path"`
	Type      string          `json:"type"`
	LoadedBy  []LoadSourceRef `json:"loadedBy"`
	LoadOrder int             `json:"loadOrder"`
}

type Coverage struct {
	Checked    int `json:"checked"`
	Resolved   int `json:"resolved"`
	Unresolved int `json:"unresolved"`
}

// ValidationIdentity labels one validated target inside a matrix. Ref is the
// requested ref text; the evidence behind it is always a fixed commit.
type ValidationIdentity struct {
	ID  string
	Ref string
}

// TOCValidation is the `source validate --toc` result. Field names keep the
// legacy contract; ResolvedCommit is always the fixed evidence commit and no
// step ever falls back to a moving ref.
type TOCValidation struct {
	ID             string                 `json:"id,omitempty"`
	Valid          bool                   `json:"valid"`
	Path           string                 `json:"path"`
	SourceID       string                 `json:"sourceId"`
	Product        string                 `json:"product"`
	Ref            string                 `json:"ref"`
	RequestedRef   string                 `json:"requestedRef"`
	MatchedTag     string                 `json:"matchedTag"`
	ResolvedCommit string                 `json:"resolvedCommit"`
	TOC            string                 `json:"toc"`
	Interface      string                 `json:"interface"`
	CheckedLua     int                    `json:"checkedLua"`
	CheckedXML     int                    `json:"checkedXml"`
	LoadClosure    []LoadFileRef          `json:"loadClosure"`
	Diagnostics    []ValidationDiagnostic `json:"diagnostics"`
	Unresolved     []UnresolvedItem       `json:"unresolved"`
	Coverage       Coverage               `json:"coverage"`
	Facts          []ValidationFact       `json:"facts"`
}

// ValidateTOC runs the TOC-closure validation mode: ordered XML/Script/Include
// closure with bounded cycle handling and duplicate-source traceability,
// static reference analysis, and compatibility lookup against exactly one
// pinned snapshot index. Unresolved items (dynamic references and unprovable
// categories) are reported separately from hard diagnostic errors.
func (b *Browser) ValidateTOC(ctx context.Context, pin selection.SourcePin, snapshotID string, input AddonInput, identity ValidationIdentity) (TOCValidation, error) {
	result := TOCValidation{ID: identity.ID, Valid: true, Path: filepath.Clean(input.Root), SourceID: pin.Repository, Product: pin.Product,
		Ref: identity.Ref, RequestedRef: identity.Ref, ResolvedCommit: pin.ExactCommit,
		LoadClosure: []LoadFileRef{}, Diagnostics: []ValidationDiagnostic{}, Unresolved: []UnresolvedItem{}, Facts: []ValidationFact{}}
	if identity.Ref == "" {
		result.Ref, result.RequestedRef = pin.RequestedRef, pin.RequestedRef
	}
	if b.store == nil {
		return result, errors.New("codebase.archive_required")
	}
	load, err := OpenChecker(b.store).InspectAddon(ctx, input)
	if err != nil {
		return result, err
	}
	result.TOC = load.Manifest
	for _, doc := range load.Documents {
		if doc.Path != load.Manifest {
			continue
		}
		for _, header := range doc.Facts.Headers {
			if strings.EqualFold(header.Key, "Interface") {
				result.Interface = strings.TrimSpace(header.Value)
			}
		}
	}
	db, _, err := b.openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	defer db.Close()
	var usages []ReferenceUsage
	unresolved := []UnresolvedItem{}
	for _, doc := range load.Documents {
		if doc.Path == load.Manifest {
			continue
		}
		fileType := strings.TrimPrefix(strings.ToLower(path.Ext(doc.Path)), ".")
		if fileType != "lua" && fileType != "xml" {
			continue
		}
		data, err := b.store.ReadBlob(ctx, doc.Blob, maxSourceBytes)
		if err != nil {
			return result, err
		}
		found, undecided := analyzeReferenceUsages(doc.Path, fileType, data)
		usages = append(usages, found...)
		unresolved = append(unresolved, undecided...)
	}
	usages = dedupeUsages(usages)
	facts, moreUnresolved, compatDiagnostics, err := lookupCompatibility(ctx, db, pin, snapshotID, usages, result.Interface)
	if err != nil {
		return result, err
	}
	unresolved = append(unresolved, moreUnresolved...)
	for _, issue := range load.Issues {
		result.Diagnostics = append(result.Diagnostics, validationDiagnostic(issue))
	}
	result.Diagnostics = append(result.Diagnostics, compatDiagnostics...)
	for _, doc := range load.Documents {
		if doc.Path == load.Manifest {
			continue
		}
		fileType := strings.TrimPrefix(strings.ToLower(path.Ext(doc.Path)), ".")
		file := LoadFileRef{Path: doc.Path, Type: fileType, LoadedBy: []LoadSourceRef{}, LoadOrder: len(result.LoadClosure)}
		for _, step := range load.Steps {
			if step.Path == doc.Path && step.From != "" {
				file.LoadedBy = append(file.LoadedBy, LoadSourceRef{File: step.From, Line: step.Line, Kind: step.Kind})
			}
		}
		result.LoadClosure = append(result.LoadClosure, file)
		if fileType == "lua" {
			result.CheckedLua++
		} else if fileType == "xml" {
			result.CheckedXML++
		}
	}
	checked := len(usages)
	if result.Interface != "" {
		checked++
	}
	result.Coverage = Coverage{Checked: checked, Resolved: len(facts), Unresolved: len(unresolved)}
	result.Facts = facts
	result.Unresolved = unresolved
	for _, diagnostic := range result.Diagnostics {
		if diagnostic.Severity == "error" {
			result.Valid = false
		}
	}
	return result, nil
}

// validationDiagnostic translates closure issues into the legacy diagnostic
// vocabulary while keeping 2.0 codes for conditions legacy never modeled.
// Cycles and duplicate loads keep legacy "warning" severity: they are bounded
// and traceable, and do not declare the addon invalid.
func validationDiagnostic(issue ValidationIssue) ValidationDiagnostic {
	severity, code := issue.Severity, issue.Code
	switch issue.Code {
	case "load_cycle":
		severity, code = "warning", "circular_load_reference"
	case "duplicate_load":
		severity, code = "warning", "duplicate_load_reference"
	case "load_unreadable":
		code = "load_file_unreadable"
	case "unsupported_load":
		code = "unsupported_load_type"
	case "load_reference":
		code = "load_file_unreadable"
	case "syntax":
		if strings.EqualFold(path.Ext(issue.Path), ".xml") {
			code = "xml_parse_failed"
		} else {
			code = "lua_parse_failed"
		}
	}
	evidence := map[string]any{}
	if issue.Reference != "" {
		evidence["reference"] = issue.Reference
	}
	return ValidationDiagnostic{Severity: severity, Code: code, File: issue.Path, Line: issue.Line, Column: 0, Message: issue.Message, Evidence: evidence}
}

// LuaScanDiagnostic is one syntax-only directory scan finding.
type LuaScanDiagnostic struct {
	Code    string `json:"code"`
	Message string `json:"message"`
	Path    string `json:"path"`
}

// LuaScanResult is the legacy no-TOC validate mode: a bounded, read-only,
// syntax-only Lua scan of one directory tree.
type LuaScanResult struct {
	Path        string              `json:"path"`
	SourceID    string              `json:"sourceId"`
	Product     string              `json:"product"`
	Ref         string              `json:"ref"`
	CheckedLua  int                 `json:"checkedLua"`
	Valid       bool                `json:"valid"`
	Diagnostics []LuaScanDiagnostic `json:"diagnostics"`
}

const (
	maxLuaScanFiles = 20000
	maxLuaScanBytes = 256 << 20
)

// ValidateLuaDirectory scans every Lua file under root, in deterministic
// lexical order, without following symbolic links and without writing
// anywhere. Parse failures are reported as lua_parse_failed diagnostics;
// budget overruns fail the scan explicitly instead of truncating silently.
func ValidateLuaDirectory(ctx context.Context, root string) (LuaScanResult, error) {
	result := LuaScanResult{Path: filepath.Clean(root), Valid: true, Diagnostics: []LuaScanDiagnostic{}}
	var total int64
	err := filepath.WalkDir(root, func(name string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if entry.IsDir() || entry.Type()&os.ModeSymlink != 0 || !strings.EqualFold(filepath.Ext(name), ".lua") {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return nil
		}
		result.CheckedLua++
		if result.CheckedLua > maxLuaScanFiles {
			return errors.New("codebase.directory_scan_budget")
		}
		if info.Size() > maxSourceBytes {
			return errors.New("codebase.directory_scan_budget")
		}
		total += info.Size()
		if total > maxLuaScanBytes {
			return errors.New("codebase.directory_scan_budget")
		}
		data, err := os.ReadFile(name)
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(root, name)
		if err != nil {
			relative = name
		}
		display := filepath.ToSlash(relative)
		facts, analyzeErr := AnalyzeDocument(ctx, display, data)
		if analyzeErr != nil {
			result.Valid = false
			result.Diagnostics = append(result.Diagnostics, LuaScanDiagnostic{Code: "lua_parse_failed", Message: analyzeErr.Error(), Path: display})
			return nil
		}
		for _, note := range facts.Diagnostics {
			result.Valid = false
			result.Diagnostics = append(result.Diagnostics, LuaScanDiagnostic{Code: "lua_parse_failed", Message: note.Message, Path: display})
		}
		return nil
	})
	if err != nil {
		return result, err
	}
	return result, nil
}

// MatrixTarget is one strict matrix validation target. Source is optional and
// defaults to DefaultMatrixSource; every other field is required.
type MatrixTarget struct {
	ID      string `json:"id"`
	TOC     string `json:"toc"`
	Source  string `json:"source,omitempty"`
	Product string `json:"product"`
	Ref     string `json:"ref"`
}

// DefaultMatrixSource is the documented default repository of a matrix target.
const DefaultMatrixSource = "wow-ui-source"

type MatrixConfig struct {
	Path    string         `json:"path"`
	Targets []MatrixTarget `json:"targets"`
}

// ReadMatrixConfig strictly parses one matrix file. Unknown fields are
// rejected, every target requires id, toc, product and ref, duplicate target
// ids are rejected, and the top-level path resolves relative to the matrix
// file. It returns the resolved AddOn root.
func ReadMatrixConfig(configPath string) (MatrixConfig, string, error) {
	file, err := os.Open(configPath)
	if err != nil {
		return MatrixConfig{}, "", err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, (1<<20)+1))
	if err != nil {
		return MatrixConfig{}, "", err
	}
	if len(data) > 1<<20 {
		return MatrixConfig{}, "", errors.New("codebase.invalid_matrix_config: matrix config exceeds 1 MiB")
	}
	var config MatrixConfig
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&config); err != nil {
		return MatrixConfig{}, "", errors.New("codebase.invalid_matrix_config: cannot parse matrix config: " + err.Error())
	}
	if err := decoder.Decode(new(json.RawMessage)); !errors.Is(err, io.EOF) {
		return MatrixConfig{}, "", errors.New("codebase.invalid_matrix_config: trailing data after matrix config")
	}
	if len(config.Targets) == 0 {
		return MatrixConfig{}, "", errors.New("codebase.invalid_matrix_config: matrix targets must not be empty")
	}
	seen := map[string]bool{}
	for _, target := range config.Targets {
		if target.ID == "" || target.TOC == "" || target.Product == "" || target.Ref == "" {
			return MatrixConfig{}, "", errors.New("codebase.invalid_matrix_config: each target requires id, toc, product, and ref")
		}
		if seen[target.ID] {
			return MatrixConfig{}, "", errors.New("codebase.duplicate_target_id: duplicate matrix target id: " + target.ID)
		}
		seen[target.ID] = true
	}
	base, err := filepath.Abs(filepath.Dir(configPath))
	if err != nil {
		return MatrixConfig{}, "", err
	}
	addonPath := config.Path
	if addonPath == "" {
		addonPath = "."
	}
	if !filepath.IsAbs(addonPath) {
		addonPath = filepath.Join(base, addonPath)
	}
	addonPath, err = filepath.Abs(addonPath)
	if err != nil {
		return MatrixConfig{}, "", err
	}
	return config, filepath.Clean(addonPath), nil
}

// TargetResolver resolves one matrix target to fixed evidence. The default
// resolution prepares the named ref once and returns its exact commit; tests
// and offline fixtures supply local resolution. There is no latest fallback.
type TargetResolver func(ctx context.Context, repository, product, ref string) (selection.SourcePin, error)

type FactDifference struct {
	Name    string               `json:"name"`
	Targets map[string]FactValue `json:"targets"`
}

type FactValue struct {
	Exists    bool   `json:"exists"`
	Signature string `json:"signature"`
}

type FactSummary struct {
	Shared      []string         `json:"shared"`
	Differences []FactDifference `json:"differences"`
}

type MatrixSummary struct {
	APIs                  FactSummary                       `json:"apis"`
	Events                FactSummary                       `json:"events"`
	XML                   FactSummary                       `json:"xml"`
	Interfaces            map[string]any                    `json:"interfaces"`
	SharedFiles           []string                          `json:"sharedFiles"`
	TargetOnlyFiles       map[string][]string               `json:"targetOnlyFiles"`
	SharedDiagnostics     []ValidationDiagnostic            `json:"sharedDiagnostics"`
	TargetOnlyDiagnostics map[string][]ValidationDiagnostic `json:"targetOnlyDiagnostics"`
	Unresolved            map[string][]UnresolvedItem       `json:"unresolved"`
}

// MatrixResult merges independently validated targets without changing their
// order. Every collection is initialized so the JSON shape stays stable, and
// merged diagnostics keep per-target identity in targetOnlyDiagnostics.
type MatrixResult struct {
	Path    string          `json:"path"`
	Valid   bool            `json:"valid"`
	Targets []TOCValidation `json:"targets"`
	Summary MatrixSummary   `json:"summary"`
}

func MergeMatrix(path string, targets []TOCValidation) MatrixResult {
	result := MatrixResult{
		Path:    path,
		Valid:   true,
		Targets: append([]TOCValidation{}, targets...),
		Summary: newMatrixSummary(),
	}
	for _, target := range targets {
		if !target.Valid {
			result.Valid = false
		}
		result.Summary.Unresolved[target.ID] = sortedUnresolvedItems(target.Unresolved)
		result.Summary.Interfaces[target.ID] = matrixInterfaceSummary(target)
	}
	result.Summary.APIs = mergeMatrixFacts(targets, map[string]bool{"api": true})
	result.Summary.Events = mergeMatrixFacts(targets, map[string]bool{"event": true})
	result.Summary.XML = mergeMatrixFacts(targets, map[string]bool{"template": true, "mixin": true, "frame-type": true})
	result.Summary.SharedFiles, result.Summary.TargetOnlyFiles = mergeMatrixFiles(targets)
	result.Summary.SharedDiagnostics, result.Summary.TargetOnlyDiagnostics = mergeMatrixDiagnostics(targets)
	return result
}

func newMatrixSummary() MatrixSummary {
	return MatrixSummary{
		APIs:                  FactSummary{Shared: []string{}, Differences: []FactDifference{}},
		Events:                FactSummary{Shared: []string{}, Differences: []FactDifference{}},
		XML:                   FactSummary{Shared: []string{}, Differences: []FactDifference{}},
		Interfaces:            map[string]any{},
		SharedFiles:           []string{},
		TargetOnlyFiles:       map[string][]string{},
		SharedDiagnostics:     []ValidationDiagnostic{},
		TargetOnlyDiagnostics: map[string][]ValidationDiagnostic{},
		Unresolved:            map[string][]UnresolvedItem{},
	}
}

func mergeMatrixFacts(targets []TOCValidation, includedKinds map[string]bool) FactSummary {
	summary := FactSummary{Shared: []string{}, Differences: []FactDifference{}}
	if len(targets) == 0 {
		return summary
	}
	valuesByName := map[string]map[string]FactValue{}
	for _, target := range targets {
		for _, fact := range target.Facts {
			kind := strings.ToLower(fact.Kind)
			if !includedKinds[kind] {
				continue
			}
			if valuesByName[fact.Name] == nil {
				valuesByName[fact.Name] = map[string]FactValue{}
			}
			valuesByName[fact.Name][target.ID] = FactValue{Exists: fact.Exists, Signature: fact.Signature}
		}
	}
	names := matrixSortedKeys(valuesByName)
	for _, name := range names {
		targetValues := make(map[string]FactValue, len(targets))
		var sharedValue FactValue
		shared := true
		for i, target := range targets {
			value, observed := valuesByName[name][target.ID]
			targetValues[target.ID] = value
			if i == 0 {
				sharedValue = value
			} else if value != sharedValue {
				shared = false
			}
			if !observed {
				shared = false
			}
		}
		if shared {
			summary.Shared = append(summary.Shared, name)
			continue
		}
		summary.Differences = append(summary.Differences, FactDifference{Name: name, Targets: targetValues})
	}
	return summary
}

func mergeMatrixFiles(targets []TOCValidation) ([]string, map[string][]string) {
	shared := []string{}
	targetOnly := map[string][]string{}
	if len(targets) == 0 {
		return shared, targetOnly
	}
	filesByTarget := make(map[string]map[string]struct{}, len(targets))
	allFiles := map[string]struct{}{}
	for _, target := range targets {
		files := map[string]struct{}{}
		for _, file := range target.LoadClosure {
			files[file.Path] = struct{}{}
			allFiles[file.Path] = struct{}{}
		}
		filesByTarget[target.ID] = files
		targetOnly[target.ID] = []string{}
	}
	for _, file := range matrixSortedKeys(allFiles) {
		inAll := true
		for _, target := range targets {
			if _, ok := filesByTarget[target.ID][file]; !ok {
				inAll = false
				break
			}
		}
		if inAll {
			shared = append(shared, file)
			continue
		}
		for _, target := range targets {
			if _, ok := filesByTarget[target.ID][file]; ok {
				targetOnly[target.ID] = append(targetOnly[target.ID], file)
			}
		}
	}
	return shared, targetOnly
}

func mergeMatrixDiagnostics(targets []TOCValidation) ([]ValidationDiagnostic, map[string][]ValidationDiagnostic) {
	shared := []ValidationDiagnostic{}
	targetOnly := map[string][]ValidationDiagnostic{}
	if len(targets) == 0 {
		return shared, targetOnly
	}
	byTarget := make(map[string]map[string]ValidationDiagnostic, len(targets))
	allKeys := map[string]struct{}{}
	for _, target := range targets {
		items := map[string]ValidationDiagnostic{}
		for _, diagnostic := range target.Diagnostics {
			key := matrixDiagnosticKey(diagnostic)
			if _, exists := items[key]; !exists {
				items[key] = diagnostic
			}
			allKeys[key] = struct{}{}
		}
		byTarget[target.ID] = items
		targetOnly[target.ID] = []ValidationDiagnostic{}
	}
	for _, key := range matrixSortedKeys(allKeys) {
		inAll := true
		var representative ValidationDiagnostic
		for i, target := range targets {
			diagnostic, ok := byTarget[target.ID][key]
			if !ok {
				inAll = false
				continue
			}
			if i == 0 {
				representative = diagnostic
			}
		}
		if inAll {
			shared = append(shared, representative)
			continue
		}
		for _, target := range targets {
			if diagnostic, ok := byTarget[target.ID][key]; ok {
				targetOnly[target.ID] = append(targetOnly[target.ID], diagnostic)
			}
		}
	}
	return shared, targetOnly
}

func matrixDiagnosticKey(d ValidationDiagnostic) string {
	return d.Severity + "\x00" + d.Code + "\x00" + d.File + "\x00" + padLine(d.Line) + "\x00" + padLine(d.Column) + "\x00" + d.Message
}

func padLine(value int) string {
	text := strconv.Itoa(value)
	for len(text) < 10 {
		text = "0" + text
	}
	return text
}

func matrixInterfaceSummary(target TOCValidation) map[string]any {
	facts := make([]ValidationFact, 0)
	for _, fact := range target.Facts {
		if strings.Contains(strings.ToLower(fact.Kind), "interface") {
			facts = append(facts, fact)
		}
	}
	sort.Slice(facts, func(i, j int) bool {
		left, right := facts[i], facts[j]
		if left.Kind != right.Kind {
			return left.Kind < right.Kind
		}
		if left.Name != right.Name {
			return left.Name < right.Name
		}
		if left.Exists != right.Exists {
			return !left.Exists
		}
		return left.Signature < right.Signature
	})
	statuses := make([]ValidationDiagnostic, 0)
	for _, diagnostic := range target.Diagnostics {
		if strings.Contains(strings.ToLower(diagnostic.Code), "interface") {
			statuses = append(statuses, diagnostic)
		}
	}
	sort.Slice(statuses, func(i, j int) bool {
		return matrixDiagnosticKey(statuses[i]) < matrixDiagnosticKey(statuses[j])
	})
	return map[string]any{"declared": target.Interface, "facts": facts, "diagnostics": statuses}
}

func sortedUnresolvedItems(items []UnresolvedItem) []UnresolvedItem {
	result := append([]UnresolvedItem{}, items...)
	sort.Slice(result, func(i, j int) bool {
		left, right := result[i], result[j]
		if left.Kind != right.Kind {
			return left.Kind < right.Kind
		}
		if left.Expression != right.Expression {
			return left.Expression < right.Expression
		}
		if left.File != right.File {
			return left.File < right.File
		}
		if left.Line != right.Line {
			return left.Line < right.Line
		}
		if left.Column != right.Column {
			return left.Column < right.Column
		}
		return left.Reason < right.Reason
	})
	return result
}

func matrixSortedKeys[V any](values map[string]V) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
