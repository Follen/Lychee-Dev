package relational

import (
	"strconv"
	"strings"
)

type groupPlan struct {
	query *retrieval
	calls []*term
	keys  map[*term]int
}
type groupState struct {
	values     []any
	aggregates []*aggregate
}
type grouping struct {
	plan   *groupPlan
	groups map[string]*groupState
	order  []*groupState
}
type groupOutput struct{ values, order []any }

func validateGroupCorrelations(e *evaluation, node *term, keys []*term, fields []field) error {
	if node == nil {
		return nil
	}
	if err := e.spendMatch(); err != nil {
		return err
	}
	for _, key := range keys {
		if sameExpression(node, key) {
			return nil
		}
	}
	if node.query != nil || isAggregate(node) {
		_, err := bindTerm(e, node, fields, true)
		return err
	}
	for _, arg := range node.args {
		if err := validateGroupCorrelations(e, arg, keys, fields); err != nil {
			return err
		}
	}
	return nil
}

func isAggregate(node *term) bool {
	if node.kind != "call" {
		return false
	}
	switch node.value {
	case "COUNT", "MIN", "MAX", "SUM", "AVG":
		return true
	}
	return false
}

func sameExpression(a, b *term) bool {
	if a == nil || b == nil {
		return a == b
	}
	if a.kind != b.kind || a.value != b.value || a.qualifier != b.qualifier || a.outerDepth != b.outerDepth || a.distinct != b.distinct || !sameRetrieval(a.query, b.query) || len(a.args) != len(b.args) {
		return false
	}
	for i := range a.args {
		if !sameExpression(a.args[i], b.args[i]) {
			return false
		}
	}
	return true
}

func prepareGroups(query *retrieval) (*grouping, error) {
	p := &groupPlan{query: query, keys: make(map[*term]int)}
	var inspectInput func(*term) error
	inspectInput = func(node *term) error {
		if isAggregate(node) {
			return ErrType
		}
		// Aggregates inside a subquery belong to that query, not this group.
		for _, arg := range node.args {
			if err := inspectInput(arg); err != nil {
				return err
			}
		}
		return nil
	}
	for _, key := range query.groups {
		if err := inspectInput(key); err != nil {
			return nil, err
		}
	}
	var inspectOutput func(*term) error
	inspectOutput = func(node *term) error {
		if node == nil {
			return nil
		}
		for i, key := range query.groups {
			if sameExpression(node, key) {
				p.keys[node] = i
				return nil
			}
		}
		if isAggregate(node) {
			if _, err := newAggregate(node); err != nil {
				return err
			}
			if err := inspectInput(node.args[0]); err != nil {
				return err
			}
			p.calls = append(p.calls, node)
			return nil
		}
		if node.kind == "column" || node.kind == "star" {
			return ErrBinding
		}
		// Inner query syntax belongs to its own scope. Its correlations read
		// through the group-only column resolver installed during finish.
		for _, arg := range node.args {
			if err := inspectOutput(arg); err != nil {
				return err
			}
		}
		return nil
	}
	for _, output := range query.outputs {
		if err := inspectOutput(output.value); err != nil {
			return nil, err
		}
	}
	if err := inspectOutput(query.having); err != nil {
		return nil, err
	}
	for _, order := range query.order {
		if err := inspectOutput(order.value); err != nil {
			return nil, err
		}
	}
	return &grouping{plan: p, groups: make(map[string]*groupState)}, nil
}

func (g *grouping) create(e *evaluation, key string, values []any) (*groupState, error) {
	cost := int64(len(key)) + int64(len(values))*32 + int64(len(g.plan.calls))*512 + 128
	for _, v := range values {
		if text, ok := v.(string); ok {
			cost += int64(len(text))
		}
	}
	if cost > e.retainedBytes {
		return nil, ErrBudget
	}
	e.retainedBytes -= cost
	state := &groupState{values: values}
	for _, call := range g.plan.calls {
		a, err := newAggregate(call)
		if err != nil {
			return nil, err
		}
		state.aggregates = append(state.aggregates, a)
	}
	g.groups[key] = state
	g.order = append(g.order, state)
	return state, nil
}

// add consumes the current bound input row after WHERE. Only group keys and
// aggregate state survive; arbitrary representative rows are never retained.
func (g *grouping) add(e *evaluation) error {
	if err := e.spendMatch(); err != nil {
		return err
	}
	values := make([]any, len(g.plan.query.groups))
	var key strings.Builder
	for i, node := range g.plan.query.groups {
		v, err := e.value(node)
		if err != nil {
			return err
		}
		values[i] = v
		part, err := equalKey(v)
		if err != nil {
			return err
		}
		if int64(len(part)) > e.remaining {
			return ErrBudget
		}
		e.remaining -= int64(len(part))
		key.WriteString(strconv.Itoa(len(part)))
		key.WriteByte(':')
		key.WriteString(part)
		if int64(key.Len()) > e.retainedBytes {
			return ErrBudget
		}
	}
	state := g.groups[key.String()]
	if state == nil {
		var err error
		state, err = g.create(e, key.String(), values)
		if err != nil {
			return err
		}
	}
	for i, call := range g.plan.calls {
		var value any
		if !state.aggregates[i].star {
			var err error
			value, err = e.value(call.args[0])
			if err != nil {
				return err
			}
		}
		if err := state.aggregates[i].add(e, value); err != nil {
			return err
		}
	}
	return nil
}

// finish returns only complete projections. Sorting, DISTINCT and pagination
// remain query-executor stages; order expressions are evaluated here per group.
func (g *grouping) finish(e *evaluation) ([]groupOutput, error) {
	if len(g.order) == 0 && len(g.plan.query.groups) == 0 {
		if _, err := g.create(e, "", nil); err != nil {
			return nil, err
		}
	}
	previous := e.resolved
	previousColumn := e.column
	defer func() { e.resolved = previous; e.column = previousColumn }()
	var output []groupOutput
	for _, state := range g.order {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		e.resolved = make(map[*term]any)
		e.column = func(qualifier, name string) (any, error) {
			for i, key := range g.plan.query.groups {
				if key.kind == "column" && strings.EqualFold(key.value, name) && (qualifier == "" || strings.EqualFold(key.qualifier, qualifier)) {
					return state.values[i], nil
				}
			}
			return nil, ErrBinding
		}
		for node, index := range g.plan.keys {
			e.resolved[node] = state.values[index]
		}
		for i, call := range g.plan.calls {
			v, err := state.aggregates[i].result()
			if err != nil {
				return nil, err
			}
			e.resolved[call] = v
		}
		if g.plan.query.having != nil {
			v, err := e.value(g.plan.query.having)
			if err != nil {
				return nil, err
			}
			truth, err := booleanState(v)
			if err != nil {
				return nil, err
			}
			if truth != 1 {
				continue
			}
		}
		row := groupOutput{}
		for _, p := range g.plan.query.outputs {
			v, err := e.value(p.value)
			if err != nil {
				return nil, err
			}
			row.values = append(row.values, v)
		}
		for _, o := range g.plan.query.order {
			v, err := e.value(o.value)
			if err != nil {
				return nil, err
			}
			row.order = append(row.order, v)
		}
		cost := int64(64 + 32*(len(row.values)+len(row.order)))
		for _, values := range [][]any{row.values, row.order} {
			for _, v := range values {
				if text, ok := v.(string); ok {
					cost += int64(len(text))
				}
			}
		}
		if cost > e.retainedBytes {
			return nil, ErrBudget
		}
		e.retainedBytes -= cost
		output = append(output, row)
	}
	return output, nil
}
