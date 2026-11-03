package records

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/sqlengine"
	"github.com/follenfang/lycheedev/internal/vault"
	"regexp"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

type SemanticBundle struct {
	Table       string                   `json:"table"`
	Commit      string                   `json:"commit"`
	Build       string                   `json:"build"`
	Mapping     vault.BlobRef            `json:"mapping"`
	Definitions map[string]vault.BlobRef `json:"definitions"`
}
type semanticMapping struct{ field, kind, name, conditionField, conditionValue string }

var metadataName = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*$`)
var semanticFieldPattern = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]*(?:\[[0-9]+\])?$`)

func parseSemanticMappings(raw []byte, table string, budgets ...*resource.Budget) ([]semanticMapping, error) {
	if len(raw) > 4<<20 || !utf8.Valid(raw) {
		return nil, ErrDefinitionIdentity
	}
	budget := semanticParseBudget(budgets)
	if err := budget.Charge(resource.Cost{RetainedBytes: int64(len(raw)), MetadataBytes: int64(len(raw))}); err != nil {
		return nil, err
	}
	result := []semanticMapping{}
	for remaining := string(raw); remaining != ""; {
		line, tail, _ := strings.Cut(remaining, "\n")
		remaining = tail
		code, _, _ := strings.Cut(line, "//")
		parts := semanticFields(code, 5)
		if len(parts) < 2 {
			continue
		}
		tab, field, ok := strings.Cut(parts[1], "::")
		if !ok || !strings.EqualFold(tab, table) {
			continue
		}
		if parts[0] == "COLOR" {
			continue
		}
		if len(parts) < 3 || len(parts) > 4 || (parts[0] != "FLAGS" && parts[0] != "ENUM") || !metadataName.MatchString(parts[2]) || !semanticFieldPattern.MatchString(field) {
			return nil, ErrDefinitionIdentity
		}
		m := semanticMapping{field: field, kind: parts[0], name: parts[2]}
		if len(parts) == 4 {
			lhs, value, ok := strings.Cut(parts[3], "=")
			ct, cf, has := strings.Cut(lhs, "::")
			if !ok || !has || !metadataName.MatchString(ct) || !semanticFieldPattern.MatchString(cf) || len(value) > 128 {
				return nil, ErrDefinitionIdentity
			}
			// The condition remains explicit metadata, never silently applied to a row.
			m.conditionField = ct + "::" + cf
			m.conditionValue = value
		}
		if len(result) >= 4096 {
			return nil, ErrMetadataLimit
		}
		if err := budget.Charge(resource.Cost{RetainedBytes: 256, MetadataBytes: 256, DecodeWork: 1}); err != nil {
			return nil, err
		}
		result = append(result, m)
	}
	return result, nil
}

func parseSemanticValues(ctx context.Context, raw []byte, build string, budgets ...*resource.Budget) ([][]any, error) {
	if len(raw) > 4<<20 || !utf8.Valid(raw) {
		return nil, ErrDefinitionIdentity
	}
	budget := semanticParseBudget(budgets)
	if err := budget.Charge(resource.Cost{RetainedBytes: int64(len(raw)), MetadataBytes: int64(len(raw))}); err != nil {
		return nil, err
	}
	rows := [][]any{}
	type selectedValue struct {
		index     int
		qualified bool
	}
	selected := map[any]selectedValue{}
	for remaining := string(raw); remaining != ""; {
		line, tail, _ := strings.Cut(remaining, "\n")
		remaining = tail
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		code, comment, _ := strings.Cut(line, "//")
		code = strings.TrimSpace(code)
		if code == "" {
			continue
		}
		qualified := strings.HasPrefix(code, "(")
		if qualified {
			annotation, rest, ok := strings.Cut(code, ")")
			if !ok || !strings.HasPrefix(annotation, "(BUILD ") {
				return nil, ErrDefinitionIdentity
			}
			matches, err := matchesSemanticBuild(build, strings.TrimPrefix(annotation, "(BUILD "))
			if err != nil {
				return nil, err
			}
			if !matches {
				continue
			}
			code = strings.TrimSpace(rest)
		}
		valueText, nameText := code, ""
		if end := strings.IndexFunc(code, unicode.IsSpace); end >= 0 {
			valueText, nameText = code[:end], strings.TrimSpace(code[end:])
		}
		base := 10
		if strings.HasPrefix(valueText, "0x") {
			base = 16
			valueText = valueText[2:]
		}
		var value any
		if strings.HasPrefix(valueText, "-") {
			n, err := strconv.ParseInt(valueText, base, 64)
			if err != nil {
				return nil, ErrDefinitionIdentity
			}
			value = n
		} else {
			n, err := strconv.ParseUint(valueText, base, 64)
			if err != nil {
				return nil, ErrDefinitionIdentity
			}
			value = n
		}
		var name any
		if nameText != "" {
			name = nameText
		}
		if prior, ok := selected[value]; ok && (prior.qualified || qualified) {
			if err := budget.Charge(resource.Cost{DecodeWork: 1}); err != nil {
				return nil, err
			}
			if qualified {
				rows[prior.index] = []any{value, name, strings.TrimSpace(comment)}
				selected[value] = selectedValue{prior.index, true}
			}
			continue
		}
		if len(rows) >= 100000 {
			return nil, ErrMetadataLimit
		}
		// Include the value-selection index used for BUILD overrides.
		if err := budget.Charge(resource.Cost{RetainedBytes: 224, MetadataBytes: 224, DecodeWork: 1}); err != nil {
			return nil, err
		}
		rows = append(rows, []any{value, name, strings.TrimSpace(comment)})
		selected[value] = selectedValue{len(rows) - 1, qualified}
	}
	return rows, nil
}

// Metadata presets are fixed by WoWDBDefs 541f07fd4bf0e3ade0b6bafcadc1343dd699adb2,
// code/C#/DBDefsLib/BuildRange.cs. Keep them outside the DBD layout parser.
func matchesSemanticBuild(build, constraints string) (bool, error) {
	presets := map[string]string{
		"Pre":     "0.3.4.2953-0.12.0.3988",
		"Vanilla": "1.0.0.3980-1.12.3.6141",
		"TBC":     "2.0.0.5610-2.4.3.8606",
		"WotLK":   "3.0.0.8120-3.3.5.12340",
		"Cata":    "4.0.0.11927-4.3.4.15595",
		"MoP":     "5.0.1.15464-5.4.8.18414",
		"WoD":     "6.0.1.18125-6.2.4.21742",
		"Legion":  "7.0.1.20740-7.3.5.26972",
		"BfA":     "8.0.1.25703-8.3.7.35662",
		"SL":      "9.0.1.33411-9.2.7.45745",
		"DF":      "10.0.0.43342-10.2.7.55664",
		"TWW":     "11.0.0.54311-11.2.7.65299",
	}
	parts := strings.Split(constraints, ",")
	for i, part := range parts {
		if replacement, ok := presets[strings.TrimSpace(part)]; ok {
			parts[i] = replacement
		}
	}
	return schema.MatchesBuild(build, strings.Join(parts, ","))
}

func semanticParseBudget(budgets []*resource.Budget) *resource.Budget {
	if len(budgets) == 0 {
		return nil
	}
	return budgets[0]
}

// Only the bounded number of fields needed to reject invalid syntax is kept.
// A malformed large line cannot allocate one string slot per token.
func semanticFields(code string, maximum int) []string {
	parts := make([]string, 0, maximum)
	for field := range strings.FieldsSeq(code) {
		parts = append(parts, field)
		if len(parts) == maximum {
			break
		}
	}
	return parts
}

// SemanticSource supplies pinned enum/flags metadata as meta.TableName SQL
// rows. Raw DB2 columns remain unchanged. A missing mapping is an empty catalog,
// not a claim that all possible values/flags are known.
func (d *Definitions) SemanticSource(ctx context.Context, commit, build, table string, offline bool) (sqlengine.Source, SemanticBundle, error) {
	definition, err := d.Prepare(ctx, commit, table, offline)
	if err != nil {
		return sqlengine.Source{}, SemanticBundle{}, err
	}
	table = definition.Identity.Name
	base := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + commit + "/meta/"
	bundle := SemanticBundle{Table: table, Commit: commit, Build: build, Definitions: map[string]vault.BlobRef{}}
	var mappings []semanticMapping
	bundle.Mapping, err = d.load(ctx, base+"mapping.dbdm", offline, func(raw []byte) error {
		var err error
		mappings, err = parseSemanticMappings(raw, table, d.budget)
		return err
	})
	if err != nil {
		return sqlengine.Source{}, bundle, err
	}
	all := [][]any{}
	cache := map[string][][]any{}
	var retained int64
	for _, m := range mappings {
		path := "enums/" + m.name + ".dbde"
		if m.kind == "FLAGS" {
			path = "flags/" + m.name + ".dbdf"
		}
		values, ok := cache[path]
		if !ok {
			if len(cache) >= 128 {
				return sqlengine.Source{}, bundle, ErrMetadataLimit
			}
			ref, err := d.load(ctx, base+path, offline, func(raw []byte) error {
				var err error
				values, err = parseSemanticValues(ctx, raw, build, d.budget)
				return err
			})
			if err != nil {
				return sqlengine.Source{}, bundle, fmt.Errorf("metadata %s: %w", path, err)
			}
			retained += ref.Bytes
			if retained > 16<<20 {
				return sqlengine.Source{}, bundle, ErrMetadataLimit
			}
			bundle.Definitions[path] = ref
			cache[path] = values
		}
		for _, value := range values {
			if err := d.budget.Charge(resource.Cost{RetainedBytes: 256, MetadataBytes: 256, DecodeWork: 1}); err != nil {
				return sqlengine.Source{}, bundle, err
			}
			all = append(all, []any{m.field, m.kind, m.name, value[0], value[1], value[2], m.conditionField, m.conditionValue})
			if len(all) > 100000 {
				return sqlengine.Source{}, bundle, ErrMetadataLimit
			}
		}
	}
	return sqlengine.Source{Columns: []string{"Field", "Kind", "Definition", "Value", "Name", "Comment", "ConditionField", "ConditionValue"}, Types: []sqlengine.Kind{sqlengine.Text, sqlengine.Text, sqlengine.Text, sqlengine.WideInteger, sqlengine.Text, sqlengine.Text, sqlengine.Text, sqlengine.Text}, Scan: func(ctx context.Context, y func([]any) error) error {
		for _, row := range all {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := y(row); err != nil {
				return err
			}
		}
		return nil
	}}, bundle, nil
}
