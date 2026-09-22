package codebase

import (
	"context"
	"database/sql"
	"errors"
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
	type lookupKey struct{ name, category string }
	cache := map[lookupKey][]DeclarationSite{}
	for _, doc := range result.Load.Documents {
		for _, edge := range doc.Facts.Relationships {
			if edge.Category != "call" && edge.Category != "event-registration" && edge.Category != "xml-inherits" && edge.Category != "xml-function" && edge.Category != "xml-method" {
				continue
			}
			if len(result.References) >= 100000 {
				return result, errors.New("codebase.reference_budget")
			}
			reference := ReferenceAssessment{Path: doc.Path, Line: edge.Line, Name: edge.To, Category: edge.Category, Status: "unresolved", Evidence: []DeclarationSite{}}
			if edge.Confidence == "dynamic-unresolved" {
				reference.Reason = "dynamic source expression"
			} else {
				key := lookupKey{edge.To, edge.Category}
				sites, ok := cache[key]
				if !ok {
					sites, err = referenceSites(ctx, db, edge.To, edge.Category)
					if err != nil {
						return result, err
					}
					cache[key] = sites
				}
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

func referenceSites(ctx context.Context, db *sql.DB, name, category string) ([]DeclarationSite, error) {
	filter := "category IN ('api-function','api-scriptobject','api-callback')"
	switch category {
	case "event-registration":
		filter = "category='api-event'"
	case "xml-inherits":
		filter = "category LIKE 'xml-%'"
	case "xml-function", "xml-method":
		filter = "category IN ('function','api-function')"
	}
	rows, err := db.QueryContext(ctx, "SELECT path,line,end_line,signature FROM entries WHERE kind='declaration' AND name=? AND "+filter+" ORDER BY path,line LIMIT 21", name)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	sites := []DeclarationSite{}
	for rows.Next() {
		var site DeclarationSite
		if err := rows.Scan(&site.Path, &site.Line, &site.EndLine, &site.Signature); err != nil {
			return nil, err
		}
		sites = append(sites, site)
	}
	if len(sites) > 20 {
		return nil, errors.New("codebase.reference_ambiguity_budget")
	}
	return sites, rows.Err()
}
