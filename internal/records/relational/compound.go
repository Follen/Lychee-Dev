package relational

import (
	"context"
	"strings"
)

type queryScope struct {
	parent *queryScope
	tables map[string]*Result
}

func (s *queryScope) lookup(name string) (*Result, bool) {
	for ; s != nil; s = s.parent {
		if result, found := s.tables[strings.ToLower(name)]; found {
			return result, true
		}
	}
	return nil, false
}

func resultSource(result Result) Source {
	return Source{Columns: result.Columns, Scan: func(ctx context.Context, yield func([]any) error) error {
		for _, row := range result.Rows {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := yield(row); err != nil {
				return err
			}
		}
		return ctx.Err()
	}}
}

func executeQuery(e *evaluation, query *retrieval, resolve Resolver, parent *queryScope) (Result, error) {
	if err := e.spendMatch(); err != nil {
		return Result{}, err
	}
	scope := &queryScope{parent: parent, tables: make(map[string]*Result)}
	// Reserve local names before executing definitions: a forward/self reference
	// must not silently fall through to an unrelated physical or outer table.
	for _, binding := range query.bindings {
		scope.tables[strings.ToLower(binding.name)] = nil
	}
	for _, binding := range query.bindings {
		var result Result
		var err error
		if query.recursive && recursiveReferences(binding.query, binding.name) > 0 {
			result, err = executeRecursive(e, binding, resolve, scope)
		} else {
			result, err = executeQuery(e, binding.query, resolve, scope)
		}
		if err != nil {
			return Result{}, err
		}
		scope.tables[strings.ToLower(binding.name)] = &result
	}
	if query.union == nil {
		return executeSelect(e, query, resolve, scope)
	}
	var combined Result
	var tail *retrieval
	for branch := query; branch != nil; branch = branch.union {
		copy := *branch
		copy.union = nil
		if branch == query {
			copy.bindings = nil
		}
		if branch.union != nil && (len(branch.order) > 0 || branch.limit != nil || branch.offset != nil) {
			return Result{}, ErrSyntax
		}
		if branch.union == nil {
			tail = branch
			copy.order = nil
			copy.limit = nil
			copy.offset = nil
		}
		result, err := executeQuery(e, &copy, resolve, scope)
		if err != nil {
			return Result{}, err
		}
		if combined.Columns == nil {
			combined.Columns = result.Columns
		} else if len(combined.Columns) != len(result.Columns) {
			return Result{}, ErrBinding
		}
		cost := int64(len(result.Rows)) * 24
		if cost > e.retainedBytes {
			return Result{}, ErrBudget
		}
		e.retainedBytes -= cost
		combined.Rows = append(combined.Rows, result.Rows...)
	}
	// Compound ORDER BY sees output names from the first branch, not source
	// columns or aliases from later branches. Its LIMIT/OFFSET apply to the union.
	final := &retrieval{order: tail.order, limit: tail.limit, offset: tail.offset}
	fields := make([]field, len(combined.Columns))
	for i, name := range combined.Columns {
		fields[i] = field{name: name}
		final.outputs = append(final.outputs, projection{value: &term{kind: "column", value: name}})
	}
	result, err := executeRows(e, final, fields, func(yield func([]any) error) error { return resultSource(combined).Scan(e.ctx, yield) })
	if err != nil {
		return Result{}, err
	}
	return Result{Columns: result.names, Rows: result.rows}, nil
}
