package environment

import (
	"bytes"
	"context"
	"encoding/json"
	"strconv"
	"strings"

	"github.com/yuin/gopher-lua/ast"
	"github.com/yuin/gopher-lua/parse"
)

func parseDocument(ctx context.Context, path string, data []byte) ([]APIRecord, []Gap) {
	records := []APIRecord{}
	gaps := []Gap{}
	nodes, err := parse.Parse(bytes.NewReader(bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})), path)
	if err != nil {
		return records, []Gap{{Path: path, Code: "parse", Detail: err.Error()}}
	}
	bindings := map[string]*ast.TableExpr{}
	ambiguous := map[string]bool{}
	for _, node := range nodes {
		if ctx.Err() != nil {
			return records, append(gaps, Gap{Path: path, Code: "cancelled"})
		}
		switch assignment := node.(type) {
		case *ast.LocalAssignStmt:
			for i, name := range assignment.Names {
				if _, exists := bindings[name]; exists {
					ambiguous[name] = true
				}
				if i < len(assignment.Exprs) {
					if literal, ok := assignment.Exprs[i].(*ast.TableExpr); ok {
						bindings[name] = literal
					} else {
						ambiguous[name] = true
					}
				} else {
					ambiguous[name] = true
				}
			}
			for _, expr := range assignment.Exprs {
				if _, literal := expr.(*ast.TableExpr); literal {
					continue
				}
				if ident, ok := expr.(*ast.IdentExpr); ok {
					if _, bound := bindings[ident.Value]; bound {
						ambiguous[ident.Value] = true
					}
					continue
				}
				for name := range bindings {
					ambiguous[name] = true
				}
			}
		case *ast.AssignStmt:
			for name := range bindings {
				ambiguous[name] = true
			}
			for _, lhs := range assignment.Lhs {
				if name := rootIdentifier(lhs); name != "" {
					if _, bound := bindings[name]; bound {
						ambiguous[name] = true
					}
				}
			}
			for _, rhs := range assignment.Rhs {
				if ident, ok := rhs.(*ast.IdentExpr); ok {
					if _, bound := bindings[ident.Value]; bound {
						ambiguous[ident.Value] = true
					}
				}
			}
		case *ast.FuncCallStmt:
		default:
			for name := range bindings {
				ambiguous[name] = true
			}
		}
		call, ok := node.(*ast.FuncCallStmt)
		if !ok {
			continue
		}
		invoke, ok := call.Expr.(*ast.FuncCallExpr)
		if !ok {
			continue
		}
		if !isDocumentationCall(invoke) {
			for name := range bindings {
				ambiguous[name] = true
			}
			for _, arg := range invoke.Args {
				if name := rootIdentifier(arg); name != "" {
					if _, bound := bindings[name]; bound {
						ambiguous[name] = true
					}
				}
			}
			continue
		}
	}
	for _, node := range nodes {
		if err := ctx.Err(); err != nil {
			return records, append(gaps, Gap{Path: path, Code: "cancelled"})
		}
		call, ok := node.(*ast.FuncCallStmt)
		if !ok {
			continue
		}
		invoke, ok := call.Expr.(*ast.FuncCallExpr)
		if !ok || !isDocumentationCall(invoke) {
			continue
		}
		root, ok := invoke.Args[0].(*ast.TableExpr)
		if !ok {
			if ident, named := invoke.Args[0].(*ast.IdentExpr); named && !ambiguous[ident.Value] {
				root, ok = bindings[ident.Value]
			}
		}
		if !ok {
			gaps = append(gaps, Gap{Path: path, Line: call.Line(), Code: "dynamic_metadata", Detail: "AddDocumentationTable argument is not an unmodified local literal"})
			continue
		}
		part, issues := readDocumentationTable(path, root)
		records = append(records, part...)
		gaps = append(gaps, issues...)
	}
	return records, gaps
}

func isDocumentationCall(invoke *ast.FuncCallExpr) bool {
	if invoke.Method != "AddDocumentationTable" || len(invoke.Args) != 1 {
		return false
	}
	receiver, ok := invoke.Receiver.(*ast.IdentExpr)
	return ok && receiver.Value == "APIDocumentation"
}

func rootIdentifier(expr ast.Expr) string {
	switch node := expr.(type) {
	case *ast.IdentExpr:
		return node.Value
	case *ast.AttrGetExpr:
		return rootIdentifier(node.Object)
	default:
		return ""
	}
}

func readDocumentationTable(path string, root *ast.TableExpr) ([]APIRecord, []Gap) {
	records := []APIRecord{}
	gaps := []Gap{}
	group := fieldString(root, "Name")
	namespace := fieldString(root, "Namespace")
	if namespace == "" && strings.HasPrefix(group, "C_") {
		// Older generated API tables encode their callable namespace in the
		// System display name. Non-C_ systems such as Unit remain globals.
		namespace = group
	}
	for _, section := range []string{"Functions", "Events", "Tables", "Predicates", "Constants"} {
		value := field(root, section)
		if value == nil {
			continue
		}
		list, ok := value.(*ast.TableExpr)
		if !ok {
			gaps = append(gaps, Gap{Path: path, Line: value.Line(), Code: "dynamic_section", Detail: section})
			continue
		}
		for _, entry := range list.Fields {
			if entry.Key != nil {
				gaps = append(gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "non_array_section", Detail: section})
				continue
			}
			table, ok := entry.Value.(*ast.TableExpr)
			if !ok {
				gaps = append(gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "dynamic_record", Detail: section})
				continue
			}
			record, issues := readRecord(path, group, namespace, section, table)
			gaps = append(gaps, issues...)
			if record.Name != "" {
				records = append(records, record)
			}
		}
	}
	return records, gaps
}

func readRecord(path, group, namespace, section string, table *ast.TableExpr) (APIRecord, []Gap) {
	name := fieldString(table, "Name")
	kind := strings.ToLower(fieldString(table, "Type"))
	if kind == "" {
		kind = strings.ToLower(strings.TrimSuffix(section, "s"))
	}
	if kind == "enum" {
		kind = "enumeration"
	}
	if kind == "scriptobject" {
		kind = "script-object"
	}
	record := APIRecord{Kind: kind, Name: name, DisplayGroup: group, Namespace: namespace, Path: path, Line: table.Line(), EndLine: table.LastLine()}
	if kind == "event" {
		if literal := fieldString(table, "LiteralName"); literal != "" {
			record.Name = literal
		}
	}
	if kind == "function" && namespace != "" {
		record.Name = namespace + "." + name
	}
	if record.EndLine < record.Line {
		record.EndLine = record.Line
	}
	gaps := []Gap{}
	if name == "" {
		gaps = append(gaps, Gap{Path: path, Line: table.Line(), Code: "missing_record_name", Detail: section})
		return APIRecord{}, gaps
	}
	record.Raw = recordRaw(path, table, map[string]bool{"Arguments": true, "Returns": true, "Payload": true, "Fields": true, "Values": true}, &gaps)
	for _, spec := range []struct {
		field  string
		target *[]Slot
	}{
		{"Arguments", &record.Parameters}, {"Returns", &record.Returns}, {"Payload", &record.Payload}, {"Fields", &record.Fields}, {"Values", &record.Values},
	} {
		value := field(table, spec.field)
		if value == nil {
			continue
		}
		list, ok := value.(*ast.TableExpr)
		if !ok {
			gaps = append(gaps, Gap{Path: path, Line: value.Line(), Code: "dynamic_slots", Detail: spec.field})
			continue
		}
		for i, entry := range list.Fields {
			if entry.Key != nil {
				gaps = append(gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "non_array_slots", Detail: spec.field})
				continue
			}
			slotTable, ok := entry.Value.(*ast.TableExpr)
			if !ok {
				gaps = append(gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "dynamic_slot", Detail: spec.field})
				continue
			}
			raw := recordRaw(path, slotTable, nil, &gaps)
			slot := Slot{Index: i + 1, Name: stringValue(raw, "Name"), Type: stringValue(raw, "Type"), InnerType: stringValue(raw, "InnerType"), Nilable: boolValue(raw, "Nilable"), Line: slotTable.Line(), Raw: raw}
			*spec.target = append(*spec.target, slot)
		}
	}
	if kind == "enumeration" && len(record.Values) == 0 {
		record.Values = record.Fields
		record.Fields = nil
	}
	return record, gaps
}

func field(table *ast.TableExpr, key string) ast.Expr {
	if table == nil {
		return nil
	}
	for _, entry := range table.Fields {
		if name, ok := entry.Key.(*ast.StringExpr); ok && name.Value == key {
			return entry.Value
		}
	}
	return nil
}
func fieldString(table *ast.TableExpr, key string) string {
	if value, ok := field(table, key).(*ast.StringExpr); ok {
		return value.Value
	}
	return ""
}
func stringValue(raw map[string]any, key string) string { value, _ := raw[key].(string); return value }
func boolValue(raw map[string]any, key string) bool     { value, _ := raw[key].(bool); return value }

func recordRaw(path string, table *ast.TableExpr, exclude map[string]bool, gaps *[]Gap) map[string]any {
	raw := map[string]any{}
	for _, entry := range table.Fields {
		name, ok := entry.Key.(*ast.StringExpr)
		if !ok {
			*gaps = append(*gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "non_named_metadata_field"})
			continue
		}
		if exclude[name.Value] {
			continue
		}
		if _, exists := raw[name.Value]; exists {
			*gaps = append(*gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "duplicate_metadata_field", Detail: name.Value})
			continue
		}
		value, supported := literal(entry.Value, 0)
		if !supported {
			*gaps = append(*gaps, Gap{Path: path, Line: entry.Value.Line(), Code: "dynamic_metadata_field", Detail: name.Value})
			continue
		}
		raw[name.Value] = value
	}
	return raw
}

func literal(expr ast.Expr, depth int) (any, bool) {
	if expr == nil || depth > 16 {
		return nil, false
	}
	switch value := expr.(type) {
	case *ast.StringExpr:
		return value.Value, true
	case *ast.NumberExpr:
		if _, err := strconv.ParseFloat(value.Value, 64); err != nil {
			return nil, false
		}
		return json.Number(value.Value), true
	case *ast.TrueExpr:
		return true, true
	case *ast.FalseExpr:
		return false, true
	case *ast.NilExpr:
		return nil, true
	case *ast.TableExpr:
		if len(value.Fields) > 4096 {
			return nil, false
		}
		object := map[string]any{}
		array := []any{}
		for _, entry := range value.Fields {
			item, ok := literal(entry.Value, depth+1)
			if !ok {
				return nil, false
			}
			if entry.Key == nil {
				if len(object) > 0 {
					return nil, false
				}
				array = append(array, item)
				continue
			}
			key, ok := entry.Key.(*ast.StringExpr)
			if !ok || len(array) > 0 {
				return nil, false
			}
			if _, exists := object[key.Value]; exists {
				return nil, false
			}
			object[key.Value] = item
		}
		if len(array) > 0 {
			return array, true
		}
		return object, true
	default:
		return nil, false
	}
}
