package flow

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/yuin/gopher-lua/ast"
	"github.com/yuin/gopher-lua/parse"
)

type function struct {
	id, name, file, owner string
	line, end             int
	params                []string
	stmts                 []ast.Stmt
	local                 bool
	conditional           bool
}

type value struct {
	state, source string
	steps         []Step
	truncated     bool
}

type frame struct {
	vars     map[string]value
	returned bool
	returns  []value
}

type analyzer struct {
	ctx           context.Context
	result        Result
	budget        Budget
	rules         RuleSet
	functions     []*function
	byName        map[string][]*function
	entries       map[string]*function
	pathCount     int
	visiting      map[string]bool
	shadowedGuard bool
}

var ErrInput = errors.New("flow.invalid_input")
var ErrBudget = errors.New("flow.budget")

func normalizeBudget(b Budget) Budget {
	if b.MaxFiles <= 0 {
		b.MaxFiles = 32
	}
	if b.MaxFunctions <= 0 {
		b.MaxFunctions = 512
	}
	if b.MaxDepth <= 0 {
		b.MaxDepth = 4
	}
	if b.MaxPaths <= 0 {
		b.MaxPaths = 128
	}
	if b.MaxMillis <= 0 {
		b.MaxMillis = 2000
	}
	if b.MaxOutputBytes <= 0 {
		b.MaxOutputBytes = 1 << 20
	}
	return b
}

// Analyze follows simple local assignments, static table fields, and uniquely
// resolved ordinary calls. A missing or unsupported edge is recorded instead
// of silently treating a value as clean. Files must be a frozen selected closure.
func Analyze(ctx context.Context, request Request) (Result, error) {
	b := normalizeBudget(request.Budget)
	result := Result{RuleIdentity: request.Rules.Identity, Findings: []Finding{}, Boundaries: append([]Boundary{}, request.Rules.Issues...), Coverage: Coverage{State: "partial"}}
	if request.Budget.MaxOutputBytes > 0 && request.Budget.MaxOutputBytes < 2048 {
		return result, fmt.Errorf("%w: output budget too small", ErrBudget)
	}
	for _, part := range []string{request.Rules.Identity.Client, request.Rules.Identity.APICommit, request.Rules.Identity.MetadataDigest, request.Rules.Identity.Version, request.Target.Symbol} {
		if len(part) > 256 {
			return result, fmt.Errorf("%w: identity or target exceeds 256 bytes", ErrBudget)
		}
	}
	if len(request.Files) == 0 || request.Target.Line < 0 {
		return result, fmt.Errorf("%w: empty input or invalid target", ErrInput)
	}
	if len(request.Files) > b.MaxFiles {
		return result, fmt.Errorf("%w: file count", ErrBudget)
	}
	if request.Target.Path != "" {
		if !validSourcePath(request.Target.Path) {
			return result, fmt.Errorf("%w: target path", ErrInput)
		}
		if _, ok := request.Files[request.Target.Path]; !ok {
			return result, fmt.Errorf("%w: target file missing", ErrInput)
		}
	}
	total := 0
	for file, raw := range request.Files {
		if !validSourcePath(file) || !strings.HasSuffix(strings.ToLower(file), ".lua") || !utf8.Valid(raw) {
			return result, fmt.Errorf("%w: invalid source file", ErrInput)
		}
		if len(file) > 512 || len(raw) > 1<<20 {
			return result, fmt.Errorf("%w: source file bytes", ErrBudget)
		}
		total += len(raw)
		if total > 32<<20 {
			return result, fmt.Errorf("%w: total source bytes", ErrBudget)
		}
	}
	bounded, cancel := context.WithTimeout(ctx, time.Duration(b.MaxMillis)*time.Millisecond)
	defer cancel()
	a := &analyzer{ctx: bounded, result: result, budget: b, rules: request.Rules, byName: map[string][]*function{}, entries: map[string]*function{}, visiting: map[string]bool{}}
	for _, file := range sortedPaths(request.Files) {
		if bounded.Err() != nil {
			a.boundary(file, 0, "time_budget", "AST preparation stopped")
			break
		}
		nodes, err := parse.Parse(bytes.NewReader(request.Files[file]), file)
		if err != nil {
			a.boundary(file, 0, "parse_error", err.Error())
			continue
		}
		a.markGuardShadow(nodes)
		entry := &function{id: file + ":<file>", name: "<file>", file: file, owner: "", line: 1, end: lineCount(request.Files[file]), stmts: nodes}
		a.entries[file] = entry
		a.collect(nodes, entry.id, file, false)
		if len(a.functions) > b.MaxFunctions {
			a.boundary(file, 0, "function_budget", "function discovery exceeded budget")
			break
		}
	}
	if len(a.entries) == 0 {
		err := a.finish()
		return a.result, err
	}
	chosen := a.selectTargets(request.Target)
	if len(chosen) == 0 {
		a.boundary(request.Target.Path, request.Target.Line, "target_unresolved", "no unique selected function or file")
		err := a.finish()
		return a.result, err
	}
	for _, fn := range chosen {
		if bounded.Err() != nil {
			a.boundary(fn.file, fn.line, "time_budget", "analysis stopped")
			break
		}
		initial := frame{vars: map[string]value{}}
		for _, parameter := range fn.params {
			initial.vars[parameter] = value{state: "unknown"}
		}
		a.exec(fn, []frame{initial}, 0)
	}
	if ctx.Err() != nil {
		return a.result, ctx.Err()
	}
	err := a.finish()
	return a.result, err
}

func validSourcePath(name string) bool {
	return name != "" && strings.ReplaceAll(name, "\\", "/") == name && path.IsAbs(name) == false && path.Clean(name) == name && name != ".." && !strings.HasPrefix(name, "../") && !strings.ContainsAny(name, ":\x00\r\n")
}

func lineCount(raw []byte) int { return bytes.Count(raw, []byte{'\n'}) + 1 }

func (a *analyzer) collect(nodes []ast.Stmt, owner, file string, block bool) {
	for _, node := range nodes {
		if len(a.functions) > a.budget.MaxFunctions || a.ctx.Err() != nil {
			return
		}
		switch n := node.(type) {
		case *ast.FuncDefStmt:
			name := expressionName(n.Name.Func)
			if n.Name.Receiver != nil {
				name = expressionName(n.Name.Receiver) + ":" + n.Name.Method
			}
			a.register(name, file, owner, n.Line(), n.LastLine(), n.Func, false, block, n.Name.Receiver != nil)
		case *ast.LocalAssignStmt:
			for i, expr := range n.Exprs {
				if i < len(n.Names) {
					if fn, ok := expr.(*ast.FunctionExpr); ok {
						localOwner := owner
						if block {
							localOwner = fmt.Sprintf("<block:%s:%d>", owner, n.Line())
						}
						a.register(n.Names[i], file, localOwner, n.Line(), n.LastLine(), fn, true, block, false)
					}
				}
			}
		case *ast.AssignStmt:
			for i, expr := range n.Rhs {
				if i < len(n.Lhs) {
					if fn, ok := expr.(*ast.FunctionExpr); ok {
						a.register(expressionName(n.Lhs[i]), file, owner, n.Line(), n.LastLine(), fn, false, block, false)
					}
				}
			}
		case *ast.IfStmt:
			a.collect(n.Then, owner, file, true)
			a.collect(n.Else, owner, file, true)
		case *ast.DoBlockStmt:
			a.collect(n.Stmts, owner, file, true)
		case *ast.WhileStmt:
			a.collect(n.Stmts, owner, file, true)
		case *ast.RepeatStmt:
			a.collect(n.Stmts, owner, file, true)
		case *ast.NumberForStmt:
			a.collect(n.Stmts, owner, file, true)
		case *ast.GenericForStmt:
			a.collect(n.Stmts, owner, file, true)
		}
	}
}

func (a *analyzer) register(name, file, owner string, line, end int, expr *ast.FunctionExpr, local, conditional, method bool) {
	if name == "" || expr == nil {
		a.boundary(file, line, "dynamic_function", "function name is not static")
		return
	}
	fn := &function{id: fmt.Sprintf("%s:%d:%s", file, line, name), name: name, file: file, owner: owner, line: line, end: end, stmts: expr.Stmts, local: local, conditional: conditional}
	if method {
		fn.params = append(fn.params, "self")
	}
	if expr.ParList != nil {
		fn.params = append(fn.params, expr.ParList.Names...)
		for _, parameter := range fn.params {
			if parameter == "issecretvalue" {
				a.shadowedGuard = true
			}
		}
	}
	a.functions = append(a.functions, fn)
	a.byName[name] = append(a.byName[name], fn)
	a.collect(expr.Stmts, fn.id, file, false)
}

func (a *analyzer) selectTargets(target Target) []*function {
	if target.Symbol != "" {
		candidates := []*function{}
		for _, fn := range a.byName[target.Symbol] {
			if target.Path == "" || fn.file == target.Path {
				candidates = append(candidates, fn)
			}
		}
		if len(candidates) > 1 {
			a.boundary(target.Path, target.Line, "ambiguous_symbol", "multiple definitions require an exact location")
			return nil
		}
		return candidates
	}
	if target.Path != "" {
		if target.Line > 0 {
			var best *function
			ambiguous := false
			for _, fn := range a.functions {
				if fn.file == target.Path && fn.line <= target.Line && target.Line <= fn.end {
					if best == nil || fn.end-fn.line < best.end-best.line {
						best = fn
						ambiguous = false
					} else if fn.end-fn.line == best.end-best.line {
						ambiguous = true
					}
				}
			}
			if ambiguous {
				a.boundary(target.Path, target.Line, "ambiguous_location", "multiple function scopes share the selected line")
				return nil
			}
			if best != nil {
				return []*function{best}
			}
		}
		if entry := a.entries[target.Path]; entry != nil {
			return []*function{entry}
		}
		return nil
	}
	// With no target, analyze only top-level statements. It is not evidence that
	// every function in the selected closure was invoked at runtime.
	files := make([]string, 0, len(a.entries))
	for file := range a.entries {
		files = append(files, file)
	}
	sort.Strings(files)
	selected := make([]*function, 0, len(files))
	for _, file := range files {
		selected = append(selected, a.entries[file])
	}
	return selected
}

func (a *analyzer) boundary(file string, line int, code, detail string) {
	if len(detail) > 256 {
		detail = detail[:256]
	}
	if len(a.result.Boundaries) < 512 {
		a.result.Boundaries = append(a.result.Boundaries, Boundary{Path: file, Line: line, Code: code, Detail: detail})
	}
	if strings.Contains(code, "budget") {
		a.result.Truncated = true
	}
}

func (a *analyzer) finish() error {
	a.result.Coverage.Files = len(a.entries)
	a.result.Coverage.Functions = len(a.functions)
	a.result.Coverage.Paths = a.pathCount
	a.result.Coverage.Complete = len(a.result.Boundaries) == 0 && !a.result.Truncated
	if a.result.Coverage.Complete {
		a.result.Coverage.State = "bounded_complete"
	} else {
		a.result.Coverage.State = "partial"
	}
	for {
		raw, _ := json.Marshal(a.result)
		if len(raw) <= a.budget.MaxOutputBytes {
			return nil
		}
		a.result.Truncated = true
		a.result.Coverage.Complete = false
		a.result.Coverage.State = "partial"
		if n := len(a.result.Findings); n > 0 {
			a.result.Findings = a.result.Findings[:n-1]
			continue
		}
		if n := len(a.result.Boundaries); n > 1 {
			a.result.Boundaries = a.result.Boundaries[:n-1]
			continue
		}
		if n := len(a.result.Boundaries); n == 1 && a.result.Boundaries[0].Detail != "" {
			a.result.Boundaries[0].Detail = ""
			continue
		}
		return fmt.Errorf("%w: minimum result exceeds output byte limit", ErrBudget)
	}
}

func expressionName(expr ast.Expr) string {
	switch e := expr.(type) {
	case *ast.IdentExpr:
		return e.Value
	case *ast.AttrGetExpr:
		base := expressionName(e.Object)
		key := literalText(e.Key)
		if base != "" && key != "" {
			return base + "." + key
		}
	}
	return ""
}

func literalText(expr ast.Expr) string {
	if value, ok := expr.(*ast.StringExpr); ok {
		return value.Value
	}
	return ""
}
