package sqlengine

import (
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"
)

// SQL syntax is parsed by DuckDB's pinned json_serialize_sql implementation.
// This file reads that tree for fixed-source admission and a narrow reader
// index hint. It neither evaluates SQL expressions nor rewrites the query.
type serializedQuery struct {
	Error        bool   `json:"error"`
	ErrorType    string `json:"error_type"`
	ErrorMessage string `json:"error_message"`
	Statements   []struct {
		Node       map[string]any `json:"node"`
		Parameters []struct {
			Key string `json:"key"`
		} `json:"named_param_map"`
	} `json:"statements"`
}

func object(value any) map[string]any {
	v, _ := value.(map[string]any)
	return v
}

func text(value any) string {
	v, _ := value.(string)
	return v
}

func items(value any) []any {
	v, _ := value.([]any)
	return v
}

func sourceKey(use TableUse) string { return strings.ToLower(use.Catalog + "." + use.Name) }

func sourceUses(root map[string]any) ([]TableUse, error) {
	found := map[string]TableUse{}
	var walk func(any, map[string]bool) error
	walk = func(value any, scope map[string]bool) error {
		switch node := value.(type) {
		case []any:
			for _, child := range node {
				if err := walk(child, scope); err != nil {
					return err
				}
			}
		case map[string]any:
			if ctes, ok := node["cte_map"]; ok {
				local := make(map[string]bool, len(scope))
				for name := range scope {
					local[name] = true
				}
				for _, item := range items(object(ctes)["map"]) {
					entry := object(item)
					if err := walk(object(entry["value"])["query_node"], local); err != nil {
						return err
					}
					local[strings.ToLower(text(entry["key"]))] = true
				}
				scope = local
			}
			if text(node["type"]) == "RECURSIVE_CTE_NODE" {
				if err := walk(node["left"], scope); err != nil {
					return err
				}
				recursive := make(map[string]bool, len(scope)+1)
				for name := range scope {
					recursive[name] = true
				}
				recursive[strings.ToLower(text(node["cte_name"]))] = true
				return walk(node["right"], recursive)
			}
			if text(node["type"]) == "BASE_TABLE" {
				name, schema := text(node["table_name"]), text(node["schema_name"])
				if schema == "" && text(node["catalog_name"]) == "" && scope[strings.ToLower(name)] {
					return nil
				}
				if schema == "" {
					schema = "static"
				}
				schema = strings.ToLower(schema)
				if text(node["catalog_name"]) != "" || (schema != "static" && schema != "effective" && schema != "meta") {
					return fmt.Errorf("%w: only pinned static, effective and meta sources are available", ErrBinding)
				}
				use := TableUse{Catalog: schema, Name: name}
				key := sourceKey(use)
				if old, ok := found[key]; !ok || name < old.Name {
					found[key] = use
				}
				if len(found) > 32 {
					return ErrBudget
				}
			}
			for key, child := range node {
				if key == "cte_map" {
					continue
				}
				if err := walk(child, scope); err != nil {
					return err
				}
			}
		}
		return nil
	}
	if err := walk(root, nil); err != nil {
		return nil, err
	}
	result := make([]TableUse, 0, len(found))
	for _, use := range found {
		result = append(result, use)
	}
	sort.Slice(result, func(i, j int) bool { return sourceKey(result[i]) < sourceKey(result[j]) })
	return result, nil
}

type identityHint struct {
	table  TableUse
	column string
	ids    []uint64
}

func indexHints(root map[string]any, parameters map[string]any) []identityHint {
	from := object(root["from_table"])
	if text(root["type"]) != "SELECT_NODE" || text(from["type"]) != "BASE_TABLE" || root["sample"] != nil || from["sample"] != nil || from["at_clause"] != nil {
		return nil
	}
	// Table column aliases can give a non-key field the original identity name.
	// Leave binding to DuckDB instead of applying a physical-ID restriction.
	if len(items(from["column_name_alias"])) != 0 {
		return nil
	}
	// A table-wide view may be referenced more than once by a CTE/subquery.
	// Only a single SELECT can safely constrain that view from its WHERE clause.
	selects := 0
	var count func(any)
	count = func(value any) {
		switch node := value.(type) {
		case map[string]any:
			if text(node["type"]) == "SELECT_NODE" {
				selects++
			}
			for _, child := range node {
				count(child)
			}
		case []any:
			for _, child := range node {
				count(child)
			}
		}
	}
	count(root)
	if selects != 1 || len(items(object(root["cte_map"])["map"])) != 0 {
		return nil
	}
	qualifier := text(from["alias"])
	if qualifier == "" {
		qualifier = text(from["table_name"])
	}
	column := func(value any) string {
		node := object(value)
		if text(node["class"]) != "COLUMN_REF" {
			return ""
		}
		names := items(node["column_names"])
		if len(names) == 1 || len(names) == 2 && strings.EqualFold(text(names[0]), qualifier) {
			return text(names[len(names)-1])
		}
		return ""
	}
	find := func(node map[string]any) (string, []uint64, bool) {
		var name string
		var values []any
		switch text(node["type"]) {
		case "COMPARE_EQUAL":
			name = column(node["left"])
			values = []any{node["right"]}
			if name == "" {
				name = column(node["right"])
				values = []any{node["left"]}
			}
		case "COMPARE_IN":
			children := items(node["children"])
			if len(children) < 2 {
				return "", nil, false
			}
			name, values = column(children[0]), children[1:]
		default:
			return "", nil, false
		}
		if name == "" || len(values) > 32768 {
			return "", nil, false
		}
		ids := make([]uint64, 0, len(values))
		seen := map[uint64]bool{}
		for _, value := range values {
			id, null, ok := constantID(object(value), parameters)
			if !ok {
				return "", nil, false
			}
			if !null && !seen[id] {
				ids = append(ids, id)
				seen[id] = true
			}
		}
		return name, ids, true
	}
	schema := text(from["schema_name"])
	if schema == "" {
		schema = "static"
	}
	var hints []identityHint
	var visit func(map[string]any)
	visit = func(node map[string]any) {
		if text(node["type"]) == "CONJUNCTION_AND" {
			for _, child := range items(node["children"]) {
				visit(object(child))
			}
		} else if name, ids, ok := find(node); ok {
			hints = append(hints, identityHint{TableUse{Catalog: schema, Name: text(from["table_name"])}, name, ids})
		}
	}
	visit(object(root["where_clause"]))
	return hints
}

func constantID(node map[string]any, parameters map[string]any) (uint64, bool, bool) {
	if text(node["class"]) == "PARAMETER" {
		value := parameters[text(node["identifier"])]
		switch v := value.(type) {
		case nil:
			return 0, true, true
		case int64:
			return uint64(v), false, v >= 0 && v <= math.MaxUint32
		case uint64:
			return v, false, v <= math.MaxUint32
		case float64:
			return uint64(v), false, v >= 0 && v <= math.MaxUint32 && v == math.Trunc(v)
		}
		return 0, false, false
	}
	if text(node["class"]) != "CONSTANT" {
		return 0, false, false
	}
	literal := object(node["literal"])
	switch text(literal["kind"]) {
	case "NULL_LITERAL":
		return 0, true, true
	case "INTEGER", "NUMERIC":
		n, ok := new(big.Rat).SetString(text(literal["text"]))
		if !ok || !n.IsInt() || !n.Num().IsUint64() || n.Num().Uint64() > math.MaxUint32 {
			return 0, false, false
		}
		return n.Num().Uint64(), false, true
	default:
		return 0, false, false
	}
}

// DuckDB serializes SELECT, not EXPLAIN. Only peel our two documented EXPLAIN
// wrappers; the complete remaining statement still goes through DuckDB.
func queryBody(sql string) (body string, explain, analyze bool) {
	body = sql
	leading := skipTrivia(sql)
	if rest, ok := takeKeyword(leading, "EXPLAIN"); ok {
		explain = true
		body = skipTrivia(rest)
		if rest, ok := takeKeyword(body, "ANALYZE"); ok {
			analyze, body = true, skipTrivia(rest)
		}
	}
	return
}

func takeKeyword(value, word string) (string, bool) {
	if len(value) < len(word) || !strings.EqualFold(value[:len(word)], word) {
		return value, false
	}
	if len(value) > len(word) {
		r, _ := utf8.DecodeRuneInString(value[len(word):])
		if unicode.IsLetter(r) || unicode.IsDigit(r) || r == '_' {
			return value, false
		}
	}
	return value[len(word):], true
}

func skipTrivia(value string) string {
	for {
		value = strings.TrimLeftFunc(value, unicode.IsSpace)
		if strings.HasPrefix(value, "--") {
			if end := strings.IndexByte(value, '\n'); end >= 0 {
				value = value[end+1:]
				continue
			}
			return ""
		}
		if !strings.HasPrefix(value, "/*") {
			return value
		}
		depth, at := 1, 2
		for at < len(value) && depth > 0 {
			switch {
			case strings.HasPrefix(value[at:], "/*"):
				depth++
				at += 2
			case strings.HasPrefix(value[at:], "*/"):
				depth--
				at += 2
			default:
				at++
			}
		}
		if depth != 0 {
			return value // Preserve malformed input for DuckDB's diagnostic.
		}
		value = value[at:]
	}
}

func decodeSyntax(raw string) (serializedQuery, error) {
	var tree serializedQuery
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(&tree); err != nil {
		return tree, err
	}
	if tree.Error {
		cause := ErrSyntax
		if tree.ErrorType == "not implemented" {
			cause = ErrReadOnly
		}
		return tree, fmt.Errorf("%w: %s", cause, tree.ErrorMessage)
	}
	if len(tree.Statements) != 1 {
		return tree, fmt.Errorf("%w: exactly one SELECT is required", ErrReadOnly)
	}
	return tree, nil
}
