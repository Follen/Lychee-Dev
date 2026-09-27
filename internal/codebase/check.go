package codebase

import (
	"context"
	"errors"
	"sort"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
)

var ErrInvalidAddon = errors.New("codebase.addon_static_check_failed")

type ReferenceAssessment struct {
	Path     string            `json:"path"`
	Line     int               `json:"line"`
	Name     string            `json:"name"`
	Category string            `json:"category"`
	Status   string            `json:"status"`
	Evidence []DeclarationSite `json:"evidence"`
	Reason   string            `json:"reason,omitempty"`
}
type CompatibilityAssessment struct {
	Semantic          *SemanticAssessment   `json:"semantic,omitempty"`
	Source            selection.SourcePin   `json:"source"`
	Load              LoadAssessment        `json:"load"`
	References        []ReferenceAssessment `json:"references"`
	ExpectedInterface int                   `json:"expectedInterface,omitempty"`
	InterfaceStatus   string                `json:"interfaceStatus"`
	Resolved          int                   `json:"resolved"`
	Unresolved        int                   `json:"unresolved"`
	StaticValid       bool                  `json:"staticValid"`
	Complete          bool                  `json:"complete"`
	Checks            []string              `json:"checks"`
	NotChecked        []string              `json:"notChecked"`
}

// CheckClosure compares static source references, not behavior, argument types,
// combat safety, or dynamic bindings. Unresolved references stay visible and
// prevent a complete result. An arbitrary addon global is not a missing API.
func (c *Checker) CheckClosure(ctx context.Context, pin selection.SourcePin, input AddonInput) (CompatibilityAssessment, error) {
	result := CompatibilityAssessment{Source: pin, References: []ReferenceAssessment{}, InterfaceStatus: "unresolved", Checks: []string{"load-paths", "load-order", "syntax", "source-name-presence", "project-interface-baseline"}, NotChecked: []string{"binding-identity", "argument-types", "dynamic-code", "combat", "taint", "runtime-behavior"}}
	db, index, err := OpenBrowser(c.store).openIndex(ctx, pin)
	if err != nil {
		return result, err
	}
	defer db.Close()
	result.Load, err = c.InspectAddon(ctx, input)
	if err != nil {
		return result, err
	}
	result.StaticValid = result.Load.LoadValid
	if expected, ok := selection.SourceInterface(pin); ok {
		result.ExpectedInterface = expected
		values := []string{}
		for _, doc := range result.Load.Documents {
			if doc.Path == result.Load.Manifest {
				for _, header := range doc.Facts.Headers {
					if strings.EqualFold(header.Key, "Interface") {
						values = append(values, header.Value)
					}
				}
			}
		}
		if len(values) == 1 {
			matched := false
			for _, value := range strings.Split(values[0], ",") {
				if strings.TrimSpace(value) == strconv.Itoa(expected) {
					matched = true
				}
			}
			if matched {
				result.InterfaceStatus = "matched"
			} else {
				result.InterfaceStatus = "mismatch"
				result.StaticValid = false
			}
		} else if len(values) > 1 {
			result.InterfaceStatus = "ambiguous"
			result.StaticValid = false
		}
	}
	wanted := map[referenceLookupKey]bool{}
	referenceCount := 0
	for _, doc := range result.Load.Documents {
		for _, edge := range doc.Facts.Relationships {
			if !checkedReferenceCategory(edge.Category) {
				continue
			}
			referenceCount++
			if referenceCount > 100000 {
				return result, errors.New("codebase.reference_budget")
			}
			if edge.Confidence != "dynamic-unresolved" {
				wanted[referenceLookupKey{edge.To, edge.Category}] = true
			}
		}
	}
	// Resolve all distinct references in one scan of the fixed index. A normal
	// addon may use hundreds of different APIs; per-name scans are quadratic.
	cache, err := collectReferenceSites(ctx, wanted, db.scan)
	if err != nil {
		return result, err
	}
	for _, doc := range result.Load.Documents {
		for _, edge := range doc.Facts.Relationships {
			if !checkedReferenceCategory(edge.Category) {
				continue
			}
			reference := ReferenceAssessment{Path: doc.Path, Line: edge.Line, Name: edge.To, Category: edge.Category, Status: "unresolved", Evidence: []DeclarationSite{}}
			if edge.Confidence == "dynamic-unresolved" {
				reference.Reason = "dynamic source expression"
			} else {
				sites := cache[referenceLookupKey{edge.To, edge.Category}]
				if len(sites) > 0 {
					reference.Status = "source-present"
					reference.Evidence = sites
				} else {
					reference.Reason = "no authoritative declaration found; may be local, dynamically supplied, or outside indexed coverage"
				}
			}
			if reference.Status == "source-present" {
				result.Resolved++
			} else {
				result.Unresolved++
			}
			result.References = append(result.References, reference)
		}
	}
	result.Complete = result.Load.LoadValid && index.Complete && result.Unresolved == 0 && result.InterfaceStatus == "matched"
	result.Load.CompatibilityComplete = result.Complete
	return result, nil
}

type referenceLookupKey struct{ name, category string }

func checkedReferenceCategory(category string) bool {
	switch category {
	case "call", "event-registration", "xml-inherits", "xml-function", "xml-method":
		return true
	}
	return false
}

func collectReferenceSites(ctx context.Context, wanted map[referenceLookupKey]bool, scan func(context.Context, func(sourceRecord) error) error) (map[referenceLookupKey][]DeclarationSite, error) {
	sites := map[referenceLookupKey][]DeclarationSite{}
	if len(wanted) == 0 {
		return sites, nil
	}
	byName := map[string][]referenceLookupKey{}
	for key := range wanted {
		byName[key.name] = append(byName[key.name], key)
	}
	err := scan(ctx, func(record sourceRecord) error {
		if record.Kind != "symbol" || record.Symbol == nil || record.Symbol.Kind != "declaration" {
			return nil
		}
		symbol := record.Symbol
		for _, key := range byName[symbol.Name] {
			eligible := symbol.Category == "api-function" || symbol.Category == "api-scriptobject" || symbol.Category == "api-callback"
			switch key.category {
			case "event-registration":
				eligible = symbol.Category == "api-event"
			case "xml-inherits":
				eligible = strings.HasPrefix(symbol.Category, "xml-")
			case "xml-function", "xml-method":
				eligible = symbol.Category == "function" || symbol.Category == "api-function"
			}
			if eligible {
				sites[key] = append(sites[key], DeclarationSite{Path: symbol.Path, Line: symbol.Line, EndLine: symbol.EndLine, Signature: symbol.Signature})
				if len(sites[key]) > 20 {
					return errors.New("codebase.reference_ambiguity_budget")
				}
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	for key := range sites {
		sort.Slice(sites[key], func(i, j int) bool {
			if sites[key][i].Path != sites[key][j].Path {
				return sites[key][i].Path < sites[key][j].Path
			}
			return sites[key][i].Line < sites[key][j].Line
		})
	}
	return sites, nil
}
