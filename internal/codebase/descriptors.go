package codebase

import (
	"strings"

	"github.com/yuin/gopher-lua/ast"
)

// Generated API metadata is read from literal AST fields. Braces in comments,
// descriptions and strings cannot alter scope or parameter section boundaries.
func (w *syntaxWalker) documentation(table *ast.TableExpr, namespace string) {
	kind, name := strings.ToLower(tableText(table, "Type")), tableText(table, "Name")
	if kind == "system" {
		namespace = tableText(table, "Namespace")
		if namespace == "" {
			namespace = name
		}
		w.declare(name, "api-system", table.Line(), table.LastLine(), name)
	} else if name != "" {
		qualified := name
		if namespace != "" {
			qualified = namespace + "." + name
		}
		switch kind {
		case "function", "event", "structure", "enumeration", "scriptobject", "callback":
			signature := descriptorSignature(table, qualified, kind)
			w.declare(qualified, "api-"+kind, table.Line(), table.LastLine(), signature)
			if namespace != "" {
				w.link(namespace, qualified, "contains", "exact", table.Line())
			}
			if literal := tableText(table, "LiteralName"); kind == "event" && literal != "" && literal != qualified {
				w.declare(literal, "api-event", table.Line(), table.LastLine(), descriptorSignature(table, literal, kind))
				w.link(literal, qualified, "event-name", "exact", table.Line())
			}
		}
	}
	// Only declared collections contain API definitions; Arguments/Returns and
	// Payload tables are parameters, never independent API declarations.
	for _, section := range []string{"Functions", "Events", "Tables", "Structures", "Enumerations", "ScriptObjects", "Callbacks"} {
		if collection, ok := tableValue(table, section).(*ast.TableExpr); ok {
			for _, entry := range collection.Fields {
				if child, ok := entry.Value.(*ast.TableExpr); ok {
					w.documentation(child, namespace)
				}
			}
		}
	}
}
func tableValue(table *ast.TableExpr, key string) ast.Expr {
	for _, field := range table.Fields {
		if literal, ok := field.Key.(*ast.StringExpr); ok && literal.Value == key {
			return field.Value
		}
	}
	return nil
}
func tableText(table *ast.TableExpr, key string) string {
	if literal, ok := tableValue(table, key).(*ast.StringExpr); ok {
		return literal.Value
	}
	return ""
}
func descriptorSignature(table *ast.TableExpr, name, kind string) string {
	parameters := func(section string) string {
		var values []string
		if list, ok := tableValue(table, section).(*ast.TableExpr); ok {
			for _, field := range list.Fields {
				item, ok := field.Value.(*ast.TableExpr)
				if !ok {
					continue
				}
				label, category := tableText(item, "Name"), tableText(item, "Type")
				if label == "" {
					label = "<dynamic>"
				}
				if category == "" {
					category = "<dynamic>"
				}
				if flag, ok := tableValue(item, "Nilable").(*ast.TrueExpr); ok && flag != nil {
					label += "?"
				}
				values = append(values, label+": "+category)
			}
		}
		return strings.Join(values, ", ")
	}
	switch kind {
	case "function", "scriptobject", "callback":
		value := name + "(" + parameters("Arguments") + ")"
		if result := parameters("Returns"); result != "" {
			value += " -> " + result
		}
		return value
	case "event":
		return name + "(" + parameters("Payload") + ")"
	default:
		return name
	}
}
