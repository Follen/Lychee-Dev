package codebase

import (
	"bytes"
	"encoding/xml"
	"strconv"
	"strings"

	"github.com/yuin/gopher-lua/ast"
	"github.com/yuin/gopher-lua/parse"
)

// ReferenceUsage is a statically resolved AddOn reference to pinned source
// data. Kinds are api, api-candidate, event, frame-type, template and mixin.
type ReferenceUsage struct {
	Kind       string `json:"kind"`
	Name       string `json:"name"`
	Expression string `json:"expression,omitempty"`
	File       string `json:"file"`
	Line       int    `json:"line"`
	Column     int    `json:"column"`
}

// UnresolvedItem reports a reference that static analysis cannot decide, such
// as computed names or categories the pinned snapshot does not prove.
type UnresolvedItem struct {
	Kind       string         `json:"kind"`
	Expression string         `json:"expression"`
	File       string         `json:"file"`
	Line       int            `json:"line"`
	Column     int            `json:"column"`
	Reason     string         `json:"reason"`
	Evidence   map[string]any `json:"evidence"`
}

// analyzeReferenceUsages extracts compatibility usages from one closure
// document without executing it. Parse failures are reported by the closure
// walk (lua_parse_failed / xml_parse_failed), so this pass only returns the
// references it could prove syntactically.
func analyzeReferenceUsages(file, fileType string, data []byte) ([]ReferenceUsage, []UnresolvedItem) {
	switch fileType {
	case "lua":
		return analyzeLuaUsages(file, data)
	case "xml":
		return analyzeXMLUsages(file, data)
	}
	return nil, nil
}

func analyzeLuaUsages(filePath string, data []byte) ([]ReferenceUsage, []UnresolvedItem) {
	clean := bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
	tree, err := parse.Parse(bytes.NewReader(clean), filePath)
	if err != nil {
		return nil, nil
	}
	var usages []ReferenceUsage
	var unresolved []UnresolvedItem
	var walkExpr func(ast.Expr)
	var walkStmts func([]ast.Stmt)
	walkExpr = func(expr ast.Expr) {
		if expr == nil {
			return
		}
		switch value := expr.(type) {
		case *ast.FuncCallExpr:
			name := callExpressionName(value)
			line := value.Line()
			if name == "" {
				unresolved = append(unresolved, unresolvedAt("api", "<dynamic call>", filePath, line, "call target is computed"))
			} else if strings.HasPrefix(name, "C_") && strings.Contains(name, ".") {
				usages = append(usages, ReferenceUsage{Kind: "api", Name: name, File: filePath, Line: line})
			} else if !strings.Contains(name, ":") {
				usages = append(usages, ReferenceUsage{Kind: "api-candidate", Name: name, File: filePath, Line: line})
			}
			switch {
			case strings.HasSuffix(name, ":RegisterEvent") || strings.HasSuffix(name, ":RegisterUnitEvent"):
				if len(value.Args) > 0 {
					if event, ok := value.Args[0].(*ast.StringExpr); ok {
						usages = append(usages, ReferenceUsage{Kind: "event", Name: event.Value, File: filePath, Line: line})
					} else {
						unresolved = append(unresolved, unresolvedAt("event", expressionLabel(value.Args[0]), filePath, line, "event name is computed"))
					}
				}
			case name == "CreateFrame":
				if len(value.Args) > 0 {
					appendLiteralUsage(&usages, &unresolved, "frame-type", value.Args[0], filePath, line)
				}
				if len(value.Args) > 3 {
					appendCSVUsages(&usages, &unresolved, "template", value.Args[3], filePath, line)
				}
			case name == "CreateFromMixins":
				for _, arg := range value.Args {
					if target := expressionLabel(arg); target != "" {
						usages = append(usages, ReferenceUsage{Kind: "mixin", Name: target, File: filePath, Line: line})
					} else {
						unresolved = append(unresolved, unresolvedAt("mixin", "<dynamic>", filePath, line, "Mixin is computed"))
					}
				}
			}
			walkExpr(value.Func)
			walkExpr(value.Receiver)
			for _, arg := range value.Args {
				walkExpr(arg)
			}
		case *ast.FunctionExpr:
			walkStmts(value.Stmts)
		case *ast.AttrGetExpr:
			walkExpr(value.Object)
			walkExpr(value.Key)
		case *ast.TableExpr:
			for _, field := range value.Fields {
				walkExpr(field.Key)
				walkExpr(field.Value)
			}
		case *ast.LogicalOpExpr:
			walkExpr(value.Lhs)
			walkExpr(value.Rhs)
		case *ast.RelationalOpExpr:
			walkExpr(value.Lhs)
			walkExpr(value.Rhs)
		case *ast.StringConcatOpExpr:
			walkExpr(value.Lhs)
			walkExpr(value.Rhs)
		case *ast.ArithmeticOpExpr:
			walkExpr(value.Lhs)
			walkExpr(value.Rhs)
		case *ast.UnaryMinusOpExpr:
			walkExpr(value.Expr)
		case *ast.UnaryNotOpExpr:
			walkExpr(value.Expr)
		case *ast.UnaryLenOpExpr:
			walkExpr(value.Expr)
		}
	}
	walkStmts = func(stmts []ast.Stmt) {
		for _, statement := range stmts {
			switch value := statement.(type) {
			case *ast.FuncDefStmt:
				walkStmts(value.Func.Stmts)
			case *ast.LocalAssignStmt:
				for _, expr := range value.Exprs {
					walkExpr(expr)
				}
			case *ast.AssignStmt:
				for _, expr := range value.Rhs {
					walkExpr(expr)
				}
			case *ast.FuncCallStmt:
				walkExpr(value.Expr)
			case *ast.DoBlockStmt:
				walkStmts(value.Stmts)
			case *ast.WhileStmt:
				walkExpr(value.Condition)
				walkStmts(value.Stmts)
			case *ast.RepeatStmt:
				walkStmts(value.Stmts)
				walkExpr(value.Condition)
			case *ast.IfStmt:
				walkExpr(value.Condition)
				walkStmts(value.Then)
				walkStmts(value.Else)
			case *ast.NumberForStmt:
				walkExpr(value.Init)
				walkExpr(value.Limit)
				walkExpr(value.Step)
				walkStmts(value.Stmts)
			case *ast.GenericForStmt:
				for _, expr := range value.Exprs {
					walkExpr(expr)
				}
				walkStmts(value.Stmts)
			case *ast.ReturnStmt:
				for _, expr := range value.Exprs {
					walkExpr(expr)
				}
			}
		}
	}
	walkStmts(tree)
	return usages, unresolved
}

func analyzeXMLUsages(filePath string, data []byte) ([]ReferenceUsage, []UnresolvedItem) {
	decoder := xml.NewDecoder(bytes.NewReader(data))
	var usages []ReferenceUsage
	var unresolved []UnresolvedItem
	for {
		offset := decoder.InputOffset()
		token, err := decoder.Token()
		if err != nil {
			break
		}
		start, ok := token.(xml.StartElement)
		if !ok {
			continue
		}
		line := 1 + bytes.Count(data[:min(int(offset), len(data))], []byte{'\n'})
		kind := strings.ToLower(start.Name.Local)
		if kind != "ui" && kind != "script" && kind != "include" && kind != "scripts" && kind != "layers" && kind != "frames" && kind != "anchors" && kind != "size" {
			usages = append(usages, ReferenceUsage{Kind: "frame-type", Name: start.Name.Local, File: filePath, Line: line})
		}
		for _, attr := range start.Attr {
			if strings.EqualFold(attr.Name.Local, "inherits") {
				for _, name := range splitCSV(attr.Value) {
					if strings.ContainsAny(name, "$%+") {
						unresolved = append(unresolved, unresolvedAt("template", name, filePath, line, "XML inheritance is dynamic"))
					} else {
						usages = append(usages, ReferenceUsage{Kind: "template", Name: name, File: filePath, Line: line})
					}
				}
			}
		}
	}
	return usages, unresolved
}

func appendLiteralUsage(usages *[]ReferenceUsage, unresolved *[]UnresolvedItem, kind string, expr ast.Expr, path string, line int) {
	if value, ok := expr.(*ast.StringExpr); ok {
		*usages = append(*usages, ReferenceUsage{Kind: kind, Name: value.Value, File: path, Line: line})
		return
	}
	*unresolved = append(*unresolved, unresolvedAt(kind, expressionLabel(expr), path, line, kind+" is computed"))
}

func appendCSVUsages(usages *[]ReferenceUsage, unresolved *[]UnresolvedItem, kind string, expr ast.Expr, path string, line int) {
	if value, ok := expr.(*ast.StringExpr); ok {
		for _, name := range splitCSV(value.Value) {
			*usages = append(*usages, ReferenceUsage{Kind: kind, Name: name, File: path, Line: line})
		}
		return
	}
	*unresolved = append(*unresolved, unresolvedAt(kind, expressionLabel(expr), path, line, kind+" is computed"))
}

func splitCSV(value string) []string {
	parts := strings.Split(value, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		if value := strings.TrimSpace(part); value != "" {
			out = append(out, value)
		}
	}
	return out
}

func unresolvedAt(kind, expression, path string, line int, reason string) UnresolvedItem {
	if expression == "" {
		expression = "<dynamic>"
	}
	return UnresolvedItem{Kind: kind, Expression: expression, File: path, Line: line, Column: 0, Reason: reason, Evidence: map[string]any{"confidence": "dynamic-unresolved"}}
}

func callExpressionName(call *ast.FuncCallExpr) string {
	if call == nil {
		return ""
	}
	if call.Receiver != nil {
		if base := expressionLabel(call.Receiver); base != "" && call.Method != "" {
			return base + ":" + call.Method
		}
	}
	return expressionLabel(call.Func)
}

func expressionLabel(expr ast.Expr) string {
	switch value := expr.(type) {
	case *ast.IdentExpr:
		return value.Value
	case *ast.StringExpr:
		return value.Value
	case *ast.AttrGetExpr:
		base, key := expressionLabel(value.Object), expressionLabel(value.Key)
		if base != "" && key != "" {
			return base + "." + key
		}
	}
	return ""
}

func dedupeUsages(input []ReferenceUsage) []ReferenceUsage {
	seen := map[string]bool{}
	out := make([]ReferenceUsage, 0, len(input))
	for _, usage := range input {
		key := usage.Kind + "\x00" + usage.Name + "\x00" + usage.File + "\x00" + strconv.Itoa(usage.Line)
		if seen[key] {
			continue
		}
		seen[key] = true
		out = append(out, usage)
	}
	return out
}
