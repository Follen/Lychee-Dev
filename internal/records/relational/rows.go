package relational

import (
	"fmt"
	"strings"
)

type field struct{ qualifier, name string }
type rowStream func(func([]any) error) error
type rowResult struct {
	names []string
	rows  [][]any
}

func fieldIndex(fields []field, qualifier, name string) (int, error) {
	index := -1
	for i, f := range fields {
		if strings.EqualFold(f.name, name) && (qualifier == "" || strings.EqualFold(f.qualifier, qualifier)) {
			if index >= 0 {
				return -1, fmt.Errorf("%w: ambiguous column %s", ErrBinding, name)
			}
			index = i
		}
	}
	if index < 0 {
		return -1, fmt.Errorf("%w: column %s.%s", ErrBinding, qualifier, name)
	}
	return index, nil
}

// bindTerm copies syntax before canonicalizing column identities. The compiled
// Program remains immutable and safe to reuse with another schema or execution.
func bindTerm(e *evaluation, node *term, fields []field, allowAggregate bool) (*term, error) {
	if node == nil {
		return nil, nil
	}
	if err := e.spendMatch(); err != nil {
		return nil, err
	}
	if node.query != nil && e.subquery == nil {
		return nil, ErrUnsupported
	}
	if node.query != nil && e.planning {
		previous := e.fields
		e.fields = fields
		result, err := e.subquery(node.query)
		e.fields = previous
		if err != nil {
			return nil, err
		}
		if node.kind != "exists" && len(result.Columns) != 1 {
			return nil, ErrCardinality
		}
	}
	copy := *node
	copy.args = nil
	if node.kind == "column" {
		bound, err := bindColumn(e, fields, node)
		if err != nil {
			return nil, err
		}
		copy = *bound
	}
	if node.kind == "parameter" {
		if _, ok := e.parameters[node.value]; !ok {
			return nil, ErrBinding
		}
	}
	if node.kind == "call" {
		if isAggregate(node) {
			if !allowAggregate {
				return nil, ErrType
			}
			if _, err := newAggregate(node); err != nil {
				return nil, err
			}
			allowAggregate = false
			// Aggregate arguments are evaluated against input rows, before the
			// grouping restriction applied to output/HAVING subqueries.
			previous := e.groupAllowed
			e.groupAllowed = nil
			defer func() { e.groupAllowed = previous }()
		} else {
			if node.distinct {
				return nil, ErrType
			}
			switch node.value {
			case "LOWER", "UPPER", "LENGTH":
				if len(node.args) != 1 {
					return nil, ErrType
				}
			case "COALESCE":
				if len(node.args) == 0 {
					return nil, ErrType
				}
			case "NULLIF":
				if len(node.args) != 2 {
					return nil, ErrType
				}
			default:
				return nil, ErrUnsupported
			}
		}
	}
	if node.kind == "cast" {
		if _, err := convertScalar(nil, node.value); err != nil {
			return nil, err
		}
	}
	for _, arg := range node.args {
		if arg.kind == "star" {
			if node.kind != "call" || node.value != "COUNT" {
				return nil, ErrType
			}
			copy.args = append(copy.args, arg)
			continue
		}
		bound, err := bindTerm(e, arg, fields, allowAggregate)
		if err != nil {
			return nil, err
		}
		copy.args = append(copy.args, bound)
	}
	return &copy, nil
}

func containsAggregate(node *term) bool {
	if node == nil {
		return false
	}
	if isAggregate(node) {
		return true
	}
	for _, arg := range node.args {
		if containsAggregate(arg) {
			return true
		}
	}
	return false
}

// executeRows is the shared filter/project/group/result pipeline over already
// bound rows. Source acquisition and JOIN/CTE expansion are separate stages.
// The stream must stop when its callback returns an error.
func executeRows(e *evaluation, query *retrieval, fields []field, stream rowStream) (rowResult, error) {
	if len(query.bindings) > 0 || len(query.links) > 0 || query.union != nil || query.input.query != nil {
		return rowResult{}, ErrUnsupported
	}
	q := *query
	q.outputs = nil
	q.groups = nil
	q.order = nil
	names := []string{}
	for _, output := range query.outputs {
		if output.value.kind == "star" {
			found := false
			for _, f := range fields {
				if output.value.qualifier != "" && !strings.EqualFold(output.value.qualifier, f.qualifier) {
					continue
				}
				found = true
				q.outputs = append(q.outputs, projection{value: &term{kind: "column", qualifier: f.qualifier, value: f.name}})
				names = append(names, f.name)
			}
			if !found || output.alias != "" {
				return rowResult{}, ErrBinding
			}
			continue
		}
		bound, err := bindTerm(e, output.value, fields, true)
		if err != nil {
			return rowResult{}, err
		}
		q.outputs = append(q.outputs, projection{value: bound, alias: output.alias})
		name := output.alias
		if name == "" {
			if bound.kind == "column" {
				name = bound.value
			} else {
				name = fmt.Sprintf("column_%d", len(names)+1)
			}
		}
		names = append(names, name)
	}
	var err error
	q.filter, err = bindTerm(e, query.filter, fields, false)
	if err != nil {
		return rowResult{}, err
	}
	q.having, err = bindTerm(e, query.having, fields, true)
	if err != nil {
		return rowResult{}, err
	}
	for _, key := range query.groups {
		bound, err := bindTerm(e, key, fields, false)
		if err != nil {
			return rowResult{}, err
		}
		q.groups = append(q.groups, bound)
	}
	for _, order := range query.order {
		value := order.value
		if value.kind == "column" && value.qualifier == "" {
			match := -1
			for i, name := range names {
				if strings.EqualFold(name, value.value) {
					if match >= 0 {
						return rowResult{}, ErrBinding
					}
					match = i
				}
			}
			if match >= 0 {
				value = q.outputs[match].value
			}
		}
		bound, err := bindTerm(e, value, fields, true)
		if err != nil {
			return rowResult{}, err
		}
		order.value = bound
		q.order = append(q.order, order)
	}
	if _, err := resultCount(e, q.limit, ^uint64(0)); err != nil {
		return rowResult{}, err
	}
	if _, err := resultCount(e, q.offset, 0); err != nil {
		return rowResult{}, err
	}
	grouped := len(q.groups) > 0 || q.having != nil
	for _, output := range q.outputs {
		grouped = grouped || containsAggregate(output.value)
	}
	for _, order := range q.order {
		grouped = grouped || containsAggregate(order.value)
	}
	var groups *grouping
	if grouped {
		groups, err = prepareGroups(&q)
		if err != nil {
			return rowResult{}, err
		}
	}
	if e.planning {
		if grouped {
			previous := e.groupAllowed
			e.groupAllowed = make(map[field]bool)
			for _, key := range q.groups {
				if key.kind == "column" {
					e.groupAllowed[field{qualifier: key.qualifier, name: key.value}] = true
				}
			}
			defer func() { e.groupAllowed = previous }()
			for _, output := range q.outputs {
				if err := validateGroupCorrelations(e, output.value, q.groups, fields); err != nil {
					return rowResult{}, err
				}
			}
			if err := validateGroupCorrelations(e, q.having, q.groups, fields); err != nil {
				return rowResult{}, err
			}
			for _, order := range q.order {
				if err := validateGroupCorrelations(e, order.value, q.groups, fields); err != nil {
					return rowResult{}, err
				}
			}
		}
		return rowResult{names: names}, nil
	}
	previousColumn, previousResolved := e.column, e.resolved
	defer func() { e.column, e.resolved = previousColumn, previousResolved }()
	e.resolved = nil
	var projected []groupOutput
	err = stream(func(input []any) error {
		if err := e.spendMatch(); err != nil {
			return err
		}
		if len(input) != len(fields) {
			return ErrBinding
		}
		e.column = func(qualifier, name string) (any, error) {
			i, err := fieldIndex(fields, qualifier, name)
			if err != nil {
				return nil, err
			}
			return input[i], nil
		}
		if q.filter != nil {
			v, err := e.value(q.filter)
			if err != nil {
				return err
			}
			truth, err := booleanState(v)
			if err != nil {
				return err
			}
			if truth != 1 {
				return nil
			}
		}
		if grouped {
			return groups.add(e)
		}
		row := groupOutput{}
		for _, output := range q.outputs {
			v, err := e.value(output.value)
			if err != nil {
				return err
			}
			row.values = append(row.values, v)
		}
		for _, order := range q.order {
			v, err := e.value(order.value)
			if err != nil {
				return err
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
			return ErrBudget
		}
		e.retainedBytes -= cost
		projected = append(projected, row)
		return nil
	})
	if err != nil {
		return rowResult{}, err
	}
	if grouped {
		projected, err = groups.finish(e)
		if err != nil {
			return rowResult{}, err
		}
	}
	projected, err = finishResults(e, &q, projected)
	if err != nil {
		return rowResult{}, err
	}
	result := rowResult{names: names, rows: make([][]any, 0, len(projected))}
	for _, row := range projected {
		result.rows = append(result.rows, row.values)
	}
	return result, nil
}
