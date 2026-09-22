package relational

import (
	"context"
	"sort"
	"strings"
)

type TableUse struct {
	Catalog string `json:"catalog"`
	Name    string `json:"name"`
}

func (p *Program) SourceTables() []TableUse  { return append([]TableUse{}, p.tables...) }
func (p *Program) NamedParameters() []string { return append([]string{}, p.parameters...) }

type nameScope struct {
	names  map[string]bool
	parent *nameScope
}

func (s *nameScope) contains(name string) bool {
	for ; s != nil; s = s.parent {
		if s.names[strings.ToLower(name)] {
			return true
		}
	}
	return false
}

type visit struct {
	query *retrieval
	value *term
	input *relation
	scope *nameScope
	depth int
}

// This walk includes expression subqueries, not just FROM/JOIN and CTE bodies.
// Linked scopes prevent an inner CTE name hiding a physical table in a sibling
// query. The iterative walk also bounds left-deep expression trees, which a
// recursion-only parser budget would miss.
func inspect(ctx context.Context, root *retrieval) ([]TableUse, []string, error) {
	work := []visit{{query: root, depth: 1}}
	tables := map[string]TableUse{}
	parameters := map[string]bool{}
	nodes := 0
	for len(work) > 0 {
		if err := ctx.Err(); err != nil {
			return nil, nil, err
		}
		item := work[len(work)-1]
		work = work[:len(work)-1]
		nodes++
		if item.depth > 128 || nodes > 32768 {
			return nil, nil, ErrBudget
		}
		pushTerm := func(value *term) {
			if value != nil {
				work = append(work, visit{value: value, scope: item.scope, depth: item.depth + 1})
			}
		}
		pushQuery := func(query *retrieval) {
			if query != nil {
				work = append(work, visit{query: query, scope: item.scope, depth: item.depth + 1})
			}
		}
		if q := item.query; q != nil {
			scope := &nameScope{names: map[string]bool{}, parent: item.scope}
			for _, binding := range q.bindings {
				scope.names[strings.ToLower(binding.name)] = true
			}
			item.scope = scope
			for _, binding := range q.bindings {
				pushQuery(binding.query)
			}
			work = append(work, visit{input: &q.input, scope: scope, depth: item.depth + 1})
			for i := range q.links {
				work = append(work, visit{input: &q.links[i].input, scope: scope, depth: item.depth + 1})
				pushTerm(q.links[i].condition)
			}
			for _, output := range q.outputs {
				pushTerm(output.value)
			}
			pushTerm(q.filter)
			pushTerm(q.having)
			pushTerm(q.limit)
			pushTerm(q.offset)
			for _, value := range q.groups {
				pushTerm(value)
			}
			for _, order := range q.order {
				pushTerm(order.value)
			}
			pushQuery(q.union)
		} else if value := item.value; value != nil {
			if value.kind == "parameter" {
				parameters[value.value] = true
			}
			for _, arg := range value.args {
				pushTerm(arg)
			}
			pushQuery(value.query)
		} else if input := item.input; input != nil {
			if input.query != nil {
				pushQuery(input.query)
				continue
			}
			if input.catalog == "" && item.scope.contains(input.name) {
				continue
			}
			catalog := strings.ToLower(input.catalog)
			if catalog == "" {
				catalog = "static"
			}
			key := catalog + "." + strings.ToLower(input.name)
			if _, exists := tables[key]; !exists {
				tables[key] = TableUse{catalog, input.name}
			}
		}
	}
	keys := make([]string, 0, len(tables))
	for key := range tables {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	result := make([]TableUse, 0, len(keys))
	for _, key := range keys {
		result = append(result, tables[key])
	}
	params := make([]string, 0, len(parameters))
	for key := range parameters {
		params = append(params, key)
	}
	sort.Strings(params)
	return result, params, nil
}
