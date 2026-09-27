package flow

import (
	"fmt"
	"strings"

	"github.com/yuin/gopher-lua/ast"
)

func cloneFrame(input frame) frame {
	copy := frame{vars: make(map[string]value, len(input.vars)), returned: input.returned, returns: append([]value{}, input.returns...)}
	for key, item := range input.vars {
		copy.vars[key] = item
	}
	return copy
}

func restoreLocals(outer frame, states []frame, body []ast.Stmt, extra ...string) []frame {
	names := append([]string{}, extra...)
	for _, node := range body {
		if local, ok := node.(*ast.LocalAssignStmt); ok {
			names = append(names, local.Names...)
		}
	}
	for i := range states {
		for _, name := range names {
			for key := range states[i].vars {
				if key == name || strings.HasPrefix(key, name+".") {
					delete(states[i].vars, key)
				}
			}
			for key, previous := range outer.vars {
				if key == name || strings.HasPrefix(key, name+".") {
					states[i].vars[key] = previous
				}
			}
		}
	}
	return states
}

func assignVar(state *frame, name string, item value) {
	for key := range state.vars {
		if strings.HasPrefix(key, name+".") {
			delete(state.vars, key)
		}
	}
	state.vars[name] = item
}

func trace(item value, step Step) value {
	copy := item
	copy.steps = append(append([]Step{}, item.steps...), step)
	if len(copy.steps) > 64 {
		copy.steps = copy.steps[len(copy.steps)-64:]
		copy.truncated = true
	}
	return copy
}

func (a *analyzer) exec(fn *function, states []frame, depth int) []frame {
	if depth > a.budget.MaxDepth {
		a.boundary(fn.file, fn.line, "depth_budget", "call depth exceeded")
		return states
	}
	if a.visiting[fn.id] {
		a.boundary(fn.file, fn.line, "recursive_call", "recursive summary is unsupported")
		return states
	}
	a.visiting[fn.id] = true
	defer delete(a.visiting, fn.id)
	return a.execStmts(fn, states, fn.stmts, depth)
}

func (a *analyzer) execStmts(fn *function, states []frame, nodes []ast.Stmt, depth int) []frame {
	for _, node := range nodes {
		if a.ctx.Err() != nil {
			a.boundary(fn.file, node.Line(), "time_budget", "analysis deadline reached")
			return states
		}
		next := make([]frame, 0, len(states))
		for _, state := range states {
			if state.returned {
				next = append(next, state)
				continue
			}
			a.pathCount++
			if a.pathCount > a.budget.MaxPaths {
				a.boundary(fn.file, node.Line(), "path_budget", "path count exceeded")
				return states
			}
			next = append(next, a.execStmt(fn, state, node, depth)...)
			if len(next) > a.budget.MaxPaths {
				a.boundary(fn.file, node.Line(), "path_budget", "branch count exceeded")
				return next[:a.budget.MaxPaths]
			}
		}
		states = next
	}
	return states
}

func (a *analyzer) execStmt(fn *function, state frame, node ast.Stmt, depth int) []frame {
	switch n := node.(type) {
	case *ast.LocalAssignStmt:
		values := a.evalAssignments(fn, state, n.Exprs, n.Line(), depth)
		for i, name := range n.Names {
			item := value{state: "unknown"}
			if i < len(values) {
				item = values[i]
			}
			assignVar(&state, name, trace(item, Step{Path: fn.file, Line: n.Line(), Kind: "assignment", Name: name, State: item.state}))
		}
	case *ast.AssignStmt:
		values := a.evalAssignments(fn, state, n.Rhs, n.Line(), depth)
		for i, lhs := range n.Lhs {
			name := expressionName(lhs)
			if name == "" {
				a.boundary(fn.file, n.Line(), "dynamic_write", "dynamic key or alias write")
				continue
			}
			item := value{state: "unknown"}
			if i < len(values) {
				item = values[i]
			}
			assignVar(&state, name, trace(item, Step{Path: fn.file, Line: n.Line(), Kind: "assignment", Name: name, State: item.state}))
		}
	case *ast.FuncCallStmt:
		if call, ok := n.Expr.(*ast.FuncCallExpr); ok {
			a.evalCall(fn, state, call, depth)
		}
	case *ast.ReturnStmt:
		state.returns = a.evalAssignments(fn, state, n.Exprs, n.Line(), depth)
		for i, item := range state.returns {
			state.returns[i] = trace(item, Step{Path: fn.file, Line: n.Line(), Kind: "return", Name: fn.name, State: item.state})
		}
		state.returned = true
	case *ast.IfStmt:
		name, negated, guard := a.guardName(fn, n.Condition)
		if !guard {
			condition := a.eval(fn, state, n.Condition, depth)[0]
			if condition.state == "secret" || condition.state == "possible" {
				a.boundary(fn.file, n.Line(), "unsupported_condition", "secret-dependent branch condition needs a version rule")
			}
		}
		left, right := cloneFrame(state), cloneFrame(state)
		if guard {
			thenSecret := !negated
			a.setGuard(&left, name, thenSecret, fn.file, n.Line())
			a.setGuard(&right, name, !thenSecret, fn.file, n.Line())
		}
		thenStates := restoreLocals(left, a.execStmts(fn, []frame{cloneFrame(left)}, n.Then, depth), n.Then)
		elseStates := restoreLocals(right, a.execStmts(fn, []frame{cloneFrame(right)}, n.Else, depth), n.Else)
		return append(thenStates, elseStates...)
	case *ast.DoBlockStmt:
		return restoreLocals(state, a.execStmts(fn, []frame{cloneFrame(state)}, n.Stmts, depth), n.Stmts)
	case *ast.WhileStmt:
		a.boundary(fn.file, n.Line(), "unsupported_loop", "loop summarized as zero or one iteration")
		one := restoreLocals(state, a.execStmts(fn, []frame{cloneFrame(state)}, n.Stmts, depth), n.Stmts)
		return append([]frame{state}, one...)
	case *ast.RepeatStmt:
		a.boundary(fn.file, n.Line(), "unsupported_loop", "repeat loop analyzed for one iteration only")
		return restoreLocals(state, a.execStmts(fn, []frame{cloneFrame(state)}, n.Stmts, depth), n.Stmts)
	case *ast.NumberForStmt:
		a.boundary(fn.file, n.Line(), "unsupported_loop", "numeric loop summarized as zero or one iteration")
		one := cloneFrame(state)
		one.vars[n.Name] = value{state: "clean"}
		return append([]frame{state}, restoreLocals(state, a.execStmts(fn, []frame{one}, n.Stmts, depth), n.Stmts, n.Name)...)
	case *ast.GenericForStmt:
		a.boundary(fn.file, n.Line(), "unsupported_loop", "generic loop summarized as zero or one iteration")
		one := cloneFrame(state)
		for _, name := range n.Names {
			one.vars[name] = value{state: "unknown"}
		}
		return append([]frame{state}, restoreLocals(state, a.execStmts(fn, []frame{one}, n.Stmts, depth), n.Stmts, n.Names...)...)
	case *ast.FuncDefStmt:
		// Registered before analysis; a definition does not execute its body.
	case *ast.BreakStmt, *ast.GotoStmt, *ast.LabelStmt:
		a.boundary(fn.file, n.Line(), "unsupported_control_flow", "break/goto control flow not modeled")
	}
	return []frame{state}
}

func (a *analyzer) evalAssignments(fn *function, state frame, exprs []ast.Expr, line, depth int) []value {
	result := []value{}
	for i, expr := range exprs {
		values := a.eval(fn, state, expr, depth)
		if len(values) == 0 {
			values = []value{{state: "unknown"}}
		}
		if i < len(exprs)-1 {
			result = append(result, values[0])
		} else {
			result = append(result, values...)
		}
	}
	return result
}

func (a *analyzer) eval(fn *function, state frame, expr ast.Expr, depth int) []value {
	if expr == nil {
		return []value{{state: "unknown"}}
	}
	switch e := expr.(type) {
	case *ast.IdentExpr, *ast.AttrGetExpr:
		if attr, ok := expr.(*ast.AttrGetExpr); ok {
			a.unknownOperation(fn, attr.Line(), "index", a.eval(fn, state, attr.Object, depth)[0], a.eval(fn, state, attr.Key, depth)[0])
		}
		name := expressionName(expr)
		if item, ok := state.vars[name]; ok {
			return []value{item}
		}
		if name != "" {
			return []value{{state: "unknown", source: name}}
		}
		a.boundary(fn.file, expr.Line(), "dynamic_read", "dynamic field read")
		return []value{{state: "unknown"}}
	case *ast.FuncCallExpr:
		return a.evalCall(fn, state, e, depth)
	case *ast.NilExpr, *ast.TrueExpr, *ast.FalseExpr, *ast.NumberExpr, *ast.StringExpr:
		return []value{{state: "clean"}}
	case *ast.TableExpr:
		item := value{state: "clean"}
		for _, field := range e.Fields {
			part := a.eval(fn, state, field.Value, depth)[0]
			if field.Key != nil {
				a.unknownOperation(fn, e.Line(), "table-key", a.eval(fn, state, field.Key, depth)[0])
			}
			item = joinValues(item, part, fn.file, e.Line(), "table-constructor")
		}
		return []value{item}
	case *ast.FunctionExpr:
		return []value{{state: "function"}}
	case *ast.LogicalOpExpr:
		left := a.eval(fn, state, e.Lhs, depth)[0]
		right := a.eval(fn, state, e.Rhs, depth)[0]
		a.unknownOperation(fn, e.Line(), "logical", left, right)
		return []value{joinValues(left, right, fn.file, e.Line(), "logical")}
	case *ast.RelationalOpExpr:
		left := a.eval(fn, state, e.Lhs, depth)[0]
		right := a.eval(fn, state, e.Rhs, depth)[0]
		a.unknownOperation(fn, e.Line(), "comparison", left, right)
		return []value{joinValues(left, right, fn.file, e.Line(), "comparison")}
	case *ast.ArithmeticOpExpr:
		left := a.eval(fn, state, e.Lhs, depth)[0]
		right := a.eval(fn, state, e.Rhs, depth)[0]
		a.unknownOperation(fn, e.Line(), "arithmetic", left, right)
		return []value{joinValues(left, right, fn.file, e.Line(), "arithmetic")}
	case *ast.StringConcatOpExpr:
		left := a.eval(fn, state, e.Lhs, depth)[0]
		right := a.eval(fn, state, e.Rhs, depth)[0]
		a.unknownOperation(fn, e.Line(), "concatenation", left, right)
		return []value{joinValues(left, right, fn.file, e.Line(), "concatenation")}
	case *ast.UnaryLenOpExpr:
		item := a.eval(fn, state, e.Expr, depth)[0]
		a.unknownOperation(fn, e.Line(), "length", item)
		return []value{trace(item, Step{Path: fn.file, Line: e.Line(), Kind: "length", State: item.state})}
	case *ast.UnaryNotOpExpr:
		item := a.eval(fn, state, e.Expr, depth)[0]
		a.unknownOperation(fn, e.Line(), "not", item)
		return []value{trace(item, Step{Path: fn.file, Line: e.Line(), Kind: "not", State: item.state})}
	case *ast.UnaryMinusOpExpr:
		item := a.eval(fn, state, e.Expr, depth)[0]
		a.unknownOperation(fn, e.Line(), "negation", item)
		return []value{trace(item, Step{Path: fn.file, Line: e.Line(), Kind: "negation", State: item.state})}
	}
	a.boundary(fn.file, expr.Line(), "unsupported_expression", fmt.Sprintf("%T", expr))
	return []value{{state: "unknown"}}
}

func (a *analyzer) unknownOperation(fn *function, line int, operation string, values ...value) {
	for _, item := range values {
		if item.state == "secret" || item.state == "possible" {
			a.boundary(fn.file, line, "unknown_operation_constraint", operation+" of a potentially secret value has no fixed client operation rule")
			return
		}
	}
}

func joinValues(left, right value, file string, line int, kind string) value {
	if left.state == "clean" {
		return trace(right, Step{Path: file, Line: line, Kind: kind, State: right.state})
	}
	if right.state == "clean" {
		return trace(left, Step{Path: file, Line: line, Kind: kind, State: left.state})
	}
	if left.state == "secret" || left.state == "possible" {
		return trace(left, Step{Path: file, Line: line, Kind: kind, State: "possible"})
	}
	return trace(right, Step{Path: file, Line: line, Kind: kind, State: "unknown"})
}

func expressionHasSecret(expr ast.Expr, vars map[string]value) bool {
	name := expressionName(expr)
	if item, ok := vars[name]; ok {
		return item.state == "secret" || item.state == "possible"
	}
	return false
}

func (a *analyzer) guardName(fn *function, condition ast.Expr) (string, bool, bool) {
	if a.shadowedGuard {
		a.boundary(fn.file, condition.Line(), "shadowed_guard", "issecretvalue is locally or globally rebound")
		return "", false, false
	}
	negated := false
	if not, ok := condition.(*ast.UnaryNotOpExpr); ok {
		negated = true
		condition = not.Expr
	}
	call, ok := condition.(*ast.FuncCallExpr)
	if !ok || len(call.Args) != 1 || expressionName(call.Func) != "issecretvalue" || call.Receiver != nil {
		return "", false, false
	}
	name := expressionName(call.Args[0])
	if name == "" {
		a.boundary(fn.file, condition.Line(), "dynamic_guard", "guard target is dynamic")
		return "", false, false
	}
	return name, negated, true
}

func (a *analyzer) setGuard(state *frame, name string, secret bool, file string, line int) {
	item, ok := state.vars[name]
	if !ok {
		item = value{state: "unknown", source: name}
	}
	if secret {
		if item.state == "possible" || item.state == "unknown" {
			item.state = "secret"
		}
	} else {
		item.state = "clean"
	}
	state.vars[name] = trace(item, Step{Path: file, Line: line, Kind: "guard", Name: name, Condition: fmt.Sprintf("issecretvalue(%s)=%t", name, secret), State: item.state})
}

func ruleUseState(arg ArgumentRule, item value) string {
	if arg.Secret == "allowed" || item.state == "clean" || item.state == "never" {
		return ""
	}
	if arg.Secret == "unknown" || item.state == "unknown" {
		return "unknown"
	}
	if arg.Secret == "forbidden" && item.state == "secret" && arg.Condition == "" {
		return "confirmed_candidate"
	}
	return "possible"
}

func (a *analyzer) evalCall(fn *function, state frame, call *ast.FuncCallExpr, depth int) []value {
	name := expressionName(call.Func)
	if call.Receiver != nil {
		name = expressionName(call.Receiver) + ":" + call.Method
	}
	args := make([]value, 0, len(call.Args))
	for i, expr := range call.Args {
		values := a.eval(fn, state, expr, depth)
		if len(values) == 0 {
			values = []value{{state: "unknown"}}
		}
		if i == len(call.Args)-1 {
			args = append(args, values...)
		} else {
			args = append(args, values[0])
		}
	}
	if name == "" {
		a.boundary(fn.file, call.Line(), "dynamic_call", "call target is not statically named")
		a.invalidateAfterUnknownCall(state, fn.file, call.Line(), "<dynamic>")
		return []value{{state: "unknown"}}
	}
	base := name
	if index := strings.IndexAny(base, ".:"); index >= 0 {
		base = base[:index]
	}
	if shadow, exists := state.vars[name]; exists && shadow.state != "function" {
		a.boundary(fn.file, call.Line(), "shadowed_call", "local/parameter value shadows "+name)
		a.invalidateAfterUnknownCall(state, fn.file, call.Line(), name)
		return []value{{state: "unknown"}}
	}
	callee, ambiguous := a.resolveFunction(fn, name, call.Line())
	if ambiguous {
		a.boundary(fn.file, call.Line(), "ambiguous_call", "multiple definitions without proven load order: "+name)
		a.invalidateAfterUnknownCall(state, fn.file, call.Line(), name)
		return []value{{state: "unknown"}}
	}
	if base != name {
		if _, exists := state.vars[base]; exists && callee == nil {
			a.boundary(fn.file, call.Line(), "shadowed_call", "local table shadows API root "+base)
			a.invalidateAfterUnknownCall(state, fn.file, call.Line(), name)
			return []value{{state: "unknown"}}
		}
	}
	if callee != nil {
		if depth >= a.budget.MaxDepth {
			a.boundary(fn.file, call.Line(), "depth_budget", "call depth exceeded at "+name)
			return []value{{state: "unknown"}}
		}
		if a.visiting[callee.id] {
			a.boundary(fn.file, call.Line(), "recursive_call", "recursive call to "+name)
			return []value{{state: "unknown"}}
		}
		callArgs := args
		if call.Receiver != nil {
			receiver := a.eval(fn, state, call.Receiver, depth)
			callArgs = append([]value{receiver[0]}, args...)
		}
		inner := frame{vars: map[string]value{}}
		for i, param := range callee.params {
			item := value{state: "unknown"}
			if i < len(callArgs) {
				item = callArgs[i]
			}
			inner.vars[param] = trace(item, Step{Path: fn.file, Line: call.Line(), Kind: "argument", Name: param, State: item.state})
		}
		states := a.exec(callee, []frame{inner}, depth+1)
		var result []value
		for _, returned := range states {
			if !returned.returned {
				a.boundary(callee.file, callee.line, "return_unknown", "function may finish without an explicit return")
				continue
			}
			if result == nil {
				result = returned.returns
			} else {
				for i, item := range returned.returns {
					if i < len(result) {
						result[i] = joinValues(result[i], item, fn.file, call.Line(), "branch-merge")
					} else {
						result = append(result, item)
					}
				}
			}
		}
		if len(result) == 0 {
			return []value{{state: "unknown"}}
		}
		for i, item := range result {
			result[i] = trace(item, Step{Path: fn.file, Line: call.Line(), Kind: "call-return", Name: name, State: item.state})
		}
		return result
	}
	rule, ok := a.rules.Calls[name]
	if !ok {
		for _, item := range args {
			if item.state == "secret" || item.state == "possible" {
				a.boundary(fn.file, call.Line(), "unknown_call_constraint", "no fixed rule or unique definition for "+name)
				break
			}
		}
		a.boundary(fn.file, call.Line(), "unresolved_call", "no fixed rule or unique definition for "+name)
		a.invalidateAfterUnknownCall(state, fn.file, call.Line(), name)
		return []value{{state: "unknown"}}
	}
	for index, argRule := range rule.Arguments {
		if index < 1 || index > len(args) {
			continue
		}
		item := args[index-1]
		findingState := ruleUseState(argRule, item)
		if findingState == "unknown" {
			if item.state != "clean" {
				a.boundary(fn.file, call.Line(), "unknown_argument_constraint", fmt.Sprintf("%s argument %d has unresolved rule/value", name, index))
			}
			continue
		}
		if findingState == "" {
			continue
		}
		if item.truncated {
			a.boundary(fn.file, call.Line(), "trace_budget", "source path exceeded 64 retained steps")
			findingState = "possible"
		}
		steps := append(append([]Step{}, item.steps...), Step{Path: fn.file, Line: call.Line(), Kind: "use", Name: name, Condition: argRule.Condition, State: findingState})
		if len(a.result.Findings) < 256 {
			a.result.Findings = append(a.result.Findings, Finding{State: findingState, Source: item.source, Use: name, Argument: index, RulePath: rule.Path, RuleLine: rule.Line, Condition: argRule.Condition, Steps: steps})
		} else {
			a.boundary(fn.file, call.Line(), "output_budget", "finding count exceeded")
		}
	}
	maxSlot := 1
	for slot := range rule.Returns {
		if slot > maxSlot {
			maxSlot = slot
		}
	}
	if maxSlot > 32 {
		a.boundary(fn.file, call.Line(), "return_slot_budget", "more than 32 return slots")
		maxSlot = 32
	}
	returns := make([]value, maxSlot)
	for i := range returns {
		returns[i] = value{state: "unknown"}
		if slot, exists := rule.Returns[i+1]; exists {
			returns[i].state = slot.State
			returns[i].source = name
			returns[i].steps = []Step{{Path: rule.Path, Line: rule.Line, Kind: "rule", Name: name, Condition: slot.Condition, State: slot.State}, {Path: fn.file, Line: call.Line(), Kind: "source", Name: name, Condition: slot.Condition, State: slot.State}}
		}
	}
	a.invalidateAfterUnknownCall(state, fn.file, call.Line(), name)
	return returns
}

func (a *analyzer) invalidateAfterUnknownCall(state frame, file string, line int, name string) {
	changed := false
	for key, item := range state.vars {
		guarded := false
		for _, step := range item.steps {
			if step.Kind == "guard" && step.State == "clean" {
				guarded = true
			}
		}
		if guarded || strings.Contains(key, ".") {
			item.state = "unknown"
			state.vars[key] = trace(item, Step{Path: file, Line: line, Kind: "unknown-side-effect", Name: name, State: "unknown"})
			changed = true
		}
	}
	if changed {
		a.boundary(file, line, "unknown_side_effect", "call may change guarded values or static fields")
	}
}

func (a *analyzer) resolveFunction(caller *function, name string, line int) (*function, bool) {
	candidates := a.byName[name]
	if len(candidates) == 0 {
		return nil, false
	}
	local := []*function{}
	global := []*function{}
	localUnresolved := false
	for _, fn := range candidates {
		if fn.local {
			if fn.file == caller.file && fn.line <= line && (fn.owner == caller.id || fn.owner == caller.owner || fn.owner == caller.file+":<file>") {
				local = append(local, fn)
			} else if fn.file == caller.file {
				localUnresolved = true
			}
		} else {
			global = append(global, fn)
		}
	}
	if len(local) == 1 {
		if local[0].conditional {
			return nil, true
		}
		return local[0], false
	}
	if len(local) > 1 {
		return nil, true
	}
	if localUnresolved {
		return nil, true
	}
	if len(global) == 1 {
		if global[0].conditional {
			return nil, true
		}
		return global[0], false
	}
	if len(global) > 1 {
		return nil, true
	}
	return nil, false
}

func (a *analyzer) markGuardShadow(nodes []ast.Stmt) {
	for _, node := range nodes {
		switch n := node.(type) {
		case *ast.LocalAssignStmt:
			for _, name := range n.Names {
				if name == "issecretvalue" {
					a.shadowedGuard = true
				}
			}
			for _, expression := range n.Exprs {
				if function, ok := expression.(*ast.FunctionExpr); ok {
					a.markGuardShadow(function.Stmts)
				}
			}
		case *ast.AssignStmt:
			for _, expr := range n.Lhs {
				if expressionName(expr) == "issecretvalue" {
					a.shadowedGuard = true
				}
			}
		case *ast.FuncDefStmt:
			if expressionName(n.Name.Func) == "issecretvalue" {
				a.shadowedGuard = true
			}
			a.markGuardShadow(n.Func.Stmts)
		case *ast.IfStmt:
			a.markGuardShadow(n.Then)
			a.markGuardShadow(n.Else)
		case *ast.DoBlockStmt:
			a.markGuardShadow(n.Stmts)
		}
	}
}
