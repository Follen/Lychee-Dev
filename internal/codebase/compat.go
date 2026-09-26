package codebase

import (
	"context"
	"database/sql"
	"errors"
	"sort"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

// ValidationFact records one checked reference against one immutable pinned
// index. Exists=false is a proven absence; unprovable categories never become
// facts and are reported as unresolved instead.
type ValidationFact struct {
	Kind      string         `json:"kind"`
	Name      string         `json:"name"`
	Exists    bool           `json:"exists"`
	Signature string         `json:"signature"`
	Evidence  map[string]any `json:"evidence"`
}

// ValidationDiagnostic is one hard validation error or warning. The five
// compatibility codes keep their legacy identifiers: api_not_found,
// event_not_found, xml_template_not_found, toc_interface_mismatch and
// compatibility_reference_not_found.
type ValidationDiagnostic struct {
	Severity string         `json:"severity"`
	Code     string         `json:"code"`
	File     string         `json:"file"`
	Line     int            `json:"line"`
	Column   int            `json:"column"`
	Message  string         `json:"message"`
	Evidence map[string]any `json:"evidence"`
}

// compatibilityMissing maps a proven absence to its diagnostic code.
func compatibilityMissing(kind, name string) (string, string) {
	switch kind {
	case "api":
		return "api_not_found", "API " + name + " does not exist in the target snapshot"
	case "event":
		return "event_not_found", "event " + name + " does not exist in the target snapshot"
	case "template":
		return "xml_template_not_found", "XML template " + name + " does not exist in the target snapshot"
	case "interface":
		return "toc_interface_mismatch", "TOC Interface " + name + " does not match the target snapshot"
	default:
		return "compatibility_reference_not_found", kind + " " + name + " does not exist in the target snapshot"
	}
}

type compatibilityMatch struct {
	Path, Role, Signature, Detail string
	Line                          int
}

// lookupCompatibility resolves static AddOn usages against one immutable
// indexed snapshot. It never falls back to facts of another snapshot or to a
// moving ref: unknown and dynamic references stay visible as unresolved.
func lookupCompatibility(ctx context.Context, db *sql.DB, pin selection.SourcePin, snapshotID string, usages []ReferenceUsage, interfaceValue string) ([]ValidationFact, []UnresolvedItem, []ValidationDiagnostic, error) {
	facts := make([]ValidationFact, 0, len(usages)+1)
	unresolved := make([]UnresolvedItem, 0)
	diagnostics := make([]ValidationDiagnostic, 0)

	interfaceSeen := false
	resolver := compatibilityResolver{ctx: ctx, db: db, pin: pin, snapshotID: snapshotID, cache: map[string]compatibilityResolution{}}
	for _, usage := range usages {
		candidate := strings.EqualFold(strings.TrimSpace(usage.Kind), "api-candidate")
		kind := normalizeCompatibilityKind(usage.Kind)
		if kind == "interface" && strings.TrimSpace(usage.Name) == strings.TrimSpace(interfaceValue) {
			interfaceSeen = true
		}
		if kind == "" {
			unresolved = append(unresolved, compatibilityUnresolved(pin, snapshotID, usage, "unsupported compatibility evidence kind"))
			continue
		}
		if strings.TrimSpace(usage.Name) == "" {
			unresolved = append(unresolved, compatibilityUnresolved(pin, snapshotID, usage, "dynamic or empty reference cannot be resolved statically"))
			continue
		}
		fact, resolved, err := resolver.lookup(usage, kind)
		if err != nil {
			return nil, nil, nil, err
		}
		if !resolved {
			unresolved = append(unresolved, compatibilityUnresolved(pin, snapshotID, usage, "the indexed snapshot cannot prove presence or absence for this reference"))
			continue
		}
		if candidate && !fact.Exists {
			unresolved = append(unresolved, compatibilityUnresolved(pin, snapshotID, usage, "the call is not an indexed API and may be an AddOn-defined global"))
			continue
		}
		facts = append(facts, fact)
		if !fact.Exists {
			code, message := compatibilityMissing(fact.Kind, fact.Name)
			diagnostics = append(diagnostics, ValidationDiagnostic{Severity: "error", Code: code, File: usage.File, Line: usage.Line, Column: usage.Column, Message: message, Evidence: fact.Evidence})
		}
	}
	if value := strings.TrimSpace(interfaceValue); value != "" && !interfaceSeen {
		usage := ReferenceUsage{Kind: "interface", Name: value, Expression: value}
		fact, resolved, err := resolver.lookup(usage, "interface")
		if err != nil {
			return nil, nil, nil, err
		}
		if resolved {
			facts = append(facts, fact)
			if !fact.Exists {
				code, message := compatibilityMissing(fact.Kind, fact.Name)
				diagnostics = append(diagnostics, ValidationDiagnostic{Severity: "error", Code: code, File: "", Line: 0, Column: 0, Message: message, Evidence: fact.Evidence})
			}
		} else {
			unresolved = append(unresolved, compatibilityUnresolved(pin, snapshotID, usage, "the indexed snapshot has no authoritative Interface evidence"))
		}
	}
	return facts, unresolved, diagnostics, nil
}

type compatibilityResolution struct {
	matches []compatibilityMatch
	known   bool
}

// compatibilityResolver caches snapshot facts, never caller evidence: each
// location keeps its own file and line even when many calls share a target.
type compatibilityResolver struct {
	ctx        context.Context
	db         *sql.DB
	pin        selection.SourcePin
	snapshotID string
	cache      map[string]compatibilityResolution
}

func (r *compatibilityResolver) lookup(usage ReferenceUsage, kind string) (ValidationFact, bool, error) {
	name := strings.TrimSpace(usage.Name)
	key := kind + "\x00" + name
	resolution, cached := r.cache[key]
	if !cached {
		matches, known, err := compatibilityMatches(r.ctx, r.db, r.pin, kind, name)
		if err != nil {
			return ValidationFact{}, false, err
		}
		resolution = compatibilityResolution{matches: matches, known: known}
		r.cache[key] = resolution
	}
	matches, categoryKnown := resolution.matches, resolution.known
	if len(matches) == 0 && (!categoryKnown || kind == "mixin" || kind == "frame-type") {
		return ValidationFact{}, false, nil
	}
	evidence := compatibilityBaseEvidence(r.pin, r.snapshotID, usage)
	evidence["matchCount"] = len(matches)
	const evidenceLimit = 5
	evidence["matchesTruncated"] = len(matches) > evidenceLimit
	evidence["matches"] = compatibilityMatchEvidence(matches[:min(len(matches), evidenceLimit)])
	if len(matches) == 0 {
		evidence["categoryPresent"] = true
	}
	signature := ""
	if len(matches) > 0 {
		signature = matches[0].Signature
	}
	return ValidationFact{Kind: kind, Name: name, Exists: len(matches) > 0, Signature: signature, Evidence: evidence}, true, nil
}

const compatibilityEvidenceLimit = 51

func compatibilityMatches(ctx context.Context, db *sql.DB, pin selection.SourcePin, kind, name string) ([]compatibilityMatch, bool, error) {
	var statement, category string
	var args []any
	switch kind {
	case "api":
		statement = `SELECT path,line,signature,category FROM entries WHERE kind='declaration' AND category IN ('api-function','api-scriptobject') AND name=? ORDER BY path,line LIMIT ?`
		args = []any{name, compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='declaration' AND category IN ('api-function','api-scriptobject'))`
	case "event":
		statement = `SELECT path,line,signature,category FROM entries WHERE kind='declaration' AND category='api-event' AND name=? ORDER BY path,line LIMIT ?`
		args = []any{name, compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='declaration' AND category='api-event')`
	case "mixin":
		statement = `SELECT path,line,signature,category FROM entries WHERE kind='declaration' AND category='mixin' AND name=? ORDER BY path,line LIMIT ?`
		args = []any{name, compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='declaration' AND category='mixin')`
	case "template":
		statement = `SELECT path,line,signature,category FROM entries WHERE kind='declaration' AND category LIKE 'xml-%' AND name=? ORDER BY path,line LIMIT ?`
		args = []any{name, compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='declaration' AND category LIKE 'xml-%' AND name<>'')`
	case "frame-type":
		statement = `SELECT path,line,signature,category FROM entries WHERE kind='declaration' AND category=? ORDER BY path,line LIMIT ?`
		args = []any{"xml-" + name, compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='declaration' AND category LIKE 'xml-%')`
	case "interface":
		statement = `SELECT path,line,target,'Interface' FROM entries WHERE kind='header' AND lower(name) LIKE 'interface%' AND target<>'' ORDER BY path,line LIMIT ?`
		args = []any{compatibilityEvidenceLimit}
		category = `SELECT EXISTS(SELECT 1 FROM entries WHERE kind='header' AND lower(name) LIKE 'interface%')`
	default:
		return nil, false, compatibilityKindError(kind)
	}
	rows, err := db.QueryContext(ctx, statement, args...)
	if err != nil {
		return nil, false, err
	}
	defer rows.Close()
	matches := make([]compatibilityMatch, 0)
	for rows.Next() {
		var match compatibilityMatch
		if err := rows.Scan(&match.Path, &match.Line, &match.Signature, &match.Detail); err != nil {
			return nil, false, err
		}
		match.Role = pathRole(match.Path)
		if kind == "interface" {
			if !interfaceValueMatches(match.Signature, name) {
				continue
			}
			match.Detail = "Interface"
		}
		matches = append(matches, match)
	}
	if err := rows.Err(); err != nil {
		return nil, false, err
	}
	var known bool
	if err := db.QueryRowContext(ctx, category).Scan(&known); err != nil {
		return nil, false, err
	}
	if kind == "interface" && pin.Repository == "wow-ui-source" {
		// A verified client baseline is authoritative Interface evidence when
		// TOC headers carry no usable line, mirroring the legacy version.txt
		// fallback without inventing build facts.
		if expected, ok := selection.SourceInterface(pin); ok {
			known = true
			if len(matches) == 0 && interfaceValueMatches(strconv.Itoa(expected), name) {
				matches = []compatibilityMatch{{Path: "selection-baseline", Line: 0, Role: "project", Signature: strconv.Itoa(expected), Detail: "Interface"}}
			}
		}
	}
	return matches, known, nil
}

// interfaceValueMatches reports whether a declared evidence value covers the
// requested Interface value. Declarations may list several builds ("50503, 50504").
func interfaceValueMatches(value, name string) bool {
	for _, part := range strings.Split(value, ",") {
		for _, requested := range strings.Split(name, ",") {
			if strings.TrimSpace(part) != "" && strings.TrimSpace(part) == strings.TrimSpace(requested) {
				return true
			}
		}
	}
	return false
}

func compatibilityKindError(kind string) error {
	return errors.New("codebase.unsupported_compatibility_kind: " + kind)
}

func normalizeCompatibilityKind(kind string) string {
	switch strings.ToLower(strings.TrimSpace(kind)) {
	case "api", "api-candidate", "function":
		return "api"
	case "event":
		return "event"
	case "template", "xml-template":
		return "template"
	case "mixin":
		return "mixin"
	case "frame-type", "frametype", "frame":
		return "frame-type"
	case "interface":
		return "interface"
	default:
		return ""
	}
}

func compatibilityUnresolved(pin selection.SourcePin, snapshotID string, usage ReferenceUsage, reason string) UnresolvedItem {
	expression := strings.TrimSpace(usage.Expression)
	if expression == "" {
		expression = strings.TrimSpace(usage.Name)
	}
	return UnresolvedItem{Kind: usage.Kind, Expression: expression, File: usage.File, Line: usage.Line, Column: usage.Column, Reason: reason, Evidence: compatibilityBaseEvidence(pin, snapshotID, usage)}
}

func compatibilityBaseEvidence(pin selection.SourcePin, snapshotID string, usage ReferenceUsage) map[string]any {
	evidence := map[string]any{
		"sourceId":       pin.Repository,
		"product":        pin.Product,
		"requestedRef":   pin.RequestedRef,
		"matchedTag":     nil,
		"resolvedCommit": pin.ExactCommit,
		"snapshotId":     snapshotID,
	}
	if usage.File != "" {
		evidence["usage"] = map[string]any{"file": usage.File, "line": usage.Line, "column": usage.Column}
	}
	return evidence
}

func compatibilityMatchEvidence(matches []compatibilityMatch) []map[string]any {
	sort.SliceStable(matches, func(i, j int) bool {
		if matches[i].Path != matches[j].Path {
			return matches[i].Path < matches[j].Path
		}
		return matches[i].Line < matches[j].Line
	})
	values := make([]map[string]any, 0, len(matches))
	for _, match := range matches {
		values = append(values, map[string]any{"path": match.Path, "line": match.Line, "role": match.Role, "signature": match.Signature, "detail": match.Detail})
	}
	return values
}
