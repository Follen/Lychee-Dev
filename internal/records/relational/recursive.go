package relational

import "strings"

// Count references using lexical CTE shadowing, not SourceTables' normalized
// catalog names: an explicitly qualified static.x is not recursive x.
func recursiveReferences(query *retrieval, name string) int {
	if query == nil {
		return 0
	}
	for _, binding := range query.bindings {
		if strings.EqualFold(binding.name, name) {
			return 0
		}
	}
	count := 0
	relationCount := func(input relation) int {
		if input.query != nil {
			return recursiveReferences(input.query, name)
		}
		if input.catalog == "" && strings.EqualFold(input.name, name) {
			return 1
		}
		return 0
	}
	count += relationCount(query.input)
	var visit func(*term)
	visit = func(node *term) {
		if node == nil {
			return
		}
		count += recursiveReferences(node.query, name)
		for _, arg := range node.args {
			visit(arg)
		}
	}
	for _, join := range query.links {
		count += relationCount(join.input)
		visit(join.condition)
	}
	for _, binding := range query.bindings {
		count += recursiveReferences(binding.query, name)
	}
	for _, output := range query.outputs {
		visit(output.value)
	}
	for _, group := range query.groups {
		visit(group)
	}
	for _, order := range query.order {
		visit(order.value)
	}
	visit(query.filter)
	visit(query.having)
	return count + recursiveReferences(query.union, name)
}

// Recursive UNION ALL is evaluated breadth-first against the previous delta,
// never against the accumulated result (which would repeat earlier paths).
// A cycle is an error at the shared budget, not a successful truncated result.
func executeRecursive(e *evaluation, binding binding, resolve Resolver, scope *queryScope) (Result, error) {
	query := binding.query
	if query.union == nil || query.union.union != nil || len(query.bindings) > 0 {
		return Result{}, ErrUnsupported
	}
	seed := *query
	seed.union = nil
	step := query.union
	if recursiveReferences(&seed, binding.name) != 0 || recursiveReferences(step, binding.name) != 1 {
		return Result{}, ErrBinding
	}
	// Per-iteration ordering/limits would alter the fixed point. Until compound
	// recursive ordering is separately planned, reject rather than misapply it.
	if len(seed.order) > 0 || seed.limit != nil || seed.offset != nil || len(step.order) > 0 || step.limit != nil || step.offset != nil {
		return Result{}, ErrUnsupported
	}
	result, err := executeQuery(e, &seed, resolve, scope)
	if err != nil {
		return Result{}, err
	}
	key := strings.ToLower(binding.name)
	previous := scope.tables[key]
	defer func() { scope.tables[key] = previous }()
	delta := result
	if e.planning {
		scope.tables[key] = &delta
		next, err := executeQuery(e, step, resolve, scope)
		if err != nil {
			return Result{}, err
		}
		if len(next.Columns) != len(result.Columns) {
			return Result{}, ErrBinding
		}
		return result, nil
	}
	for len(delta.Rows) > 0 {
		if err := e.spendMatch(); err != nil {
			return Result{}, err
		}
		scope.tables[key] = &delta
		next, err := executeQuery(e, step, resolve, scope)
		if err != nil {
			return Result{}, err
		}
		if len(next.Columns) != len(result.Columns) {
			return Result{}, ErrBinding
		}
		next.Columns = result.Columns
		cost := int64(len(next.Rows)) * 24
		if cost > e.retainedBytes {
			return Result{}, ErrBudget
		}
		e.retainedBytes -= cost
		result.Rows = append(result.Rows, next.Rows...)
		delta = next
	}
	return result, nil
}
