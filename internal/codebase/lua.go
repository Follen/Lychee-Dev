package codebase

import (
	"bytes"
	"context"
	"fmt"
	"strings"

	"github.com/yuin/gopher-lua/ast"
	"github.com/yuin/gopher-lua/parse"
)

// scanProgram consumes syntax only. It never creates a Lua VM or evaluates
// table constructors, function calls, require(), or generated API descriptors.
func scanProgram(ctx context.Context, data []byte, generated bool) DocumentFacts {
	facts := emptyFacts()
	clean := bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
	if bytes.HasPrefix(clean, []byte("#!")) {
		if end := bytes.IndexByte(clean, '\n'); end >= 0 {
			clean = append(bytes.Repeat([]byte{' '}, end), clean[end:]...)
		}
	}
	nodes, err := parse.Parse(bytes.NewReader(clean), "source")
	if err != nil {
		line := 0
		if failure, ok := err.(*parse.Error); ok && failure.Pos.Line > 0 {
			line = failure.Pos.Line
		}
		facts.Diagnostics = append(facts.Diagnostics, SyntaxNote{Message: err.Error(), Line: line})
		return facts
	}
	w := syntaxWalker{ctx: ctx, facts: &facts, lines: strings.Split(string(clean), "\n"), generated: generated, tableEnds: braceClosures(clean)}
	w.statements(nodes, "<file>")
	for _, root := range w.apiRoots {
		w.documentation(root, "")
	}
	return facts
}

type syntaxWalker struct {
	ctx       context.Context
	facts     *DocumentFacts
	lines     []string
	generated bool
	tableEnds map[int][]int
	apiRoots  []*ast.TableExpr
}

func (w *syntaxWalker) declare(name, category string, line, end int, signature string) {
	if name != "" {
		w.facts.Declarations = append(w.facts.Declarations, Declaration{Name: name, Category: category, Line: line, EndLine: end, Signature: signature})
	}
}
func (w *syntaxWalker) signature(line int) string {
	if line > 0 && line <= len(w.lines) {
		return strings.TrimSpace(w.lines[line-1])
	}
	return ""
}
func (w *syntaxWalker) link(from, to, category, confidence string, line int) {
	w.facts.Relationships = append(w.facts.Relationships, Relationship{From: from, To: to, Category: category, Confidence: confidence, Line: line})
}
func (w *syntaxWalker) statements(nodes []ast.Stmt, scope string) {
	for _, node := range nodes {
		if w.ctx.Err() != nil {
			return
		}
		switch n := node.(type) {
		case *ast.FuncDefStmt:
			name := accessName(n.Name.Func)
			if n.Name.Receiver != nil {
				name = accessName(n.Name.Receiver) + ":" + n.Name.Method
			}
			w.declare(name, "function", n.Line(), n.LastLine(), w.signature(n.Line()))
			w.statements(n.Func.Stmts, name)
		case *ast.LocalAssignStmt:
			for i, expr := range n.Exprs {
				if fn, ok := expr.(*ast.FunctionExpr); ok && i < len(n.Names) {
					w.declare(n.Names[i], "local-function", n.Line(), n.LastLine(), w.signature(n.Line()))
					w.statements(fn.Stmts, n.Names[i])
				} else {
					w.expression(expr, scope)
				}
			}
		case *ast.AssignStmt:
			for _, lhs := range n.Lhs {
				w.expression(lhs, scope)
			}
			for i, expr := range n.Rhs {
				name := ""
				if i < len(n.Lhs) {
					name = accessName(n.Lhs[i])
				}
				if fn, ok := expr.(*ast.FunctionExpr); ok && name != "" {
					w.declare(name, "function", n.Line(), n.LastLine(), w.signature(n.Line()))
					w.statements(fn.Stmts, name)
				} else {
					if call, ok := expr.(*ast.FuncCallExpr); ok && invocationName(call) == "CreateFromMixins" && name != "" {
						w.declare(name, "mixin", n.Line(), n.LastLine(), w.signature(n.Line()))
						for _, base := range call.Args {
							if parent := accessName(base); parent != "" {
								w.link(name, parent, "inherits", "inferred", n.Line())
							}
						}
					}
					w.expression(expr, scope)
				}
			}
		case *ast.FuncCallStmt:
			w.expression(n.Expr, scope)
		case *ast.DoBlockStmt:
			w.statements(n.Stmts, scope)
		case *ast.WhileStmt:
			w.expression(n.Condition, scope)
			w.statements(n.Stmts, scope)
		case *ast.RepeatStmt:
			w.statements(n.Stmts, scope)
			w.expression(n.Condition, scope)
		case *ast.IfStmt:
			w.expression(n.Condition, scope)
			w.statements(n.Then, scope)
			w.statements(n.Else, scope)
		case *ast.NumberForStmt:
			w.expression(n.Init, scope)
			w.expression(n.Limit, scope)
			w.expression(n.Step, scope)
			w.statements(n.Stmts, scope)
		case *ast.GenericForStmt:
			for _, expr := range n.Exprs {
				w.expression(expr, scope)
			}
			w.statements(n.Stmts, scope)
		case *ast.ReturnStmt:
			for _, expr := range n.Exprs {
				w.expression(expr, scope)
			}
		}
	}
}
func (w *syntaxWalker) expression(expr ast.Expr, scope string) {
	if expr == nil || w.ctx.Err() != nil {
		return
	}
	switch n := expr.(type) {
	case *ast.FuncCallExpr:
		name, confidence := invocationName(n), "inferred"
		if name == "" {
			name, confidence = "<dynamic>", "dynamic-unresolved"
		}
		w.link(scope, name, "call", confidence, n.Line())
		if strings.HasSuffix(name, ":RegisterEvent") || strings.HasSuffix(name, ":RegisterUnitEvent") {
			if len(n.Args) > 0 {
				if literal, ok := n.Args[0].(*ast.StringExpr); ok {
					w.link(scope, literal.Value, "event-registration", "exact", n.Line())
				} else {
					w.link(scope, "<dynamic>", "event-registration", "dynamic-unresolved", n.Line())
				}
			}
		}
		w.expression(n.Func, scope)
		w.expression(n.Receiver, scope)
		for _, arg := range n.Args {
			w.expression(arg, scope)
		}
	case *ast.FunctionExpr:
		w.statements(n.Stmts, fmt.Sprintf("%s/<anonymous:%d>", scope, n.Line()))
	case *ast.AttrGetExpr:
		w.expression(n.Object, scope)
		w.expression(n.Key, scope)
	case *ast.TableExpr:
		if ends := w.tableEnds[n.Line()]; len(ends) > 0 {
			n.SetLastLine(ends[0])
			w.tableEnds[n.Line()] = ends[1:]
		}
		if w.generated && tableText(n, "Type") == "System" {
			w.apiRoots = append(w.apiRoots, n)
		}
		for _, field := range n.Fields {
			w.expression(field.Key, scope)
			w.expression(field.Value, scope)
		}
	case *ast.LogicalOpExpr:
		w.expression(n.Lhs, scope)
		w.expression(n.Rhs, scope)
	case *ast.RelationalOpExpr:
		w.expression(n.Lhs, scope)
		w.expression(n.Rhs, scope)
	case *ast.StringConcatOpExpr:
		w.expression(n.Lhs, scope)
		w.expression(n.Rhs, scope)
	case *ast.ArithmeticOpExpr:
		w.expression(n.Lhs, scope)
		w.expression(n.Rhs, scope)
	case *ast.UnaryMinusOpExpr:
		w.expression(n.Expr, scope)
	case *ast.UnaryNotOpExpr:
		w.expression(n.Expr, scope)
	case *ast.UnaryLenOpExpr:
		w.expression(n.Expr, scope)
	}
}

// gopher-lua leaves table LastLine unset. Its lexer supplies matching braces
// without mistaking strings or comments for syntax. Same-line constructors
// retain lexical order, which is also the expression walk order.
func braceClosures(data []byte) map[int][]int {
	result := map[int][]int{}
	type opening struct{ line, index int }
	stack := []opening{}
	scanner := parse.NewScanner(bytes.NewReader(data), "source")
	lexer := &parse.Lexer{}
	for {
		token, err := scanner.Scan(lexer)
		if err != nil || token.Type == parse.EOF {
			break
		}
		lexer.PrevTokenType = token.Type
		switch token.Type {
		case '{':
			line := token.Pos.Line
			stack = append(stack, opening{line, len(result[line])})
			result[line] = append(result[line], line)
		case '}':
			if len(stack) > 0 {
				last := stack[len(stack)-1]
				stack = stack[:len(stack)-1]
				result[last.line][last.index] = token.Pos.Line
			}
		}
	}
	return result
}
func accessName(expr ast.Expr) string {
	switch n := expr.(type) {
	case *ast.IdentExpr:
		return n.Value
	case *ast.AttrGetExpr:
		base := accessName(n.Object)
		key, ok := n.Key.(*ast.StringExpr)
		if base != "" && ok {
			return base + "." + key.Value
		}
	}
	return ""
}
func invocationName(call *ast.FuncCallExpr) string {
	if call.Receiver != nil {
		if base := accessName(call.Receiver); base != "" {
			return base + ":" + call.Method
		}
		return ""
	}
	return accessName(call.Func)
}
