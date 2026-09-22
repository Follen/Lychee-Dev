package relational

// Structural equality intentionally ignores source offsets. Equal subqueries
// repeated in SELECT and GROUP BY must refer to the same group key, not rerun
// against an arbitrary input row. There are no cycles in compiler-owned syntax.
func sameRetrieval(a, b *retrieval) bool {
	if a == nil || b == nil {
		return a == b
	}
	if a.recursive != b.recursive || a.distinct != b.distinct || len(a.bindings) != len(b.bindings) || len(a.outputs) != len(b.outputs) || len(a.links) != len(b.links) || len(a.groups) != len(b.groups) || len(a.order) != len(b.order) {
		return false
	}
	if !sameRelation(a.input, b.input) || !sameExpression(a.filter, b.filter) || !sameExpression(a.having, b.having) || !sameExpression(a.limit, b.limit) || !sameExpression(a.offset, b.offset) || !sameRetrieval(a.union, b.union) {
		return false
	}
	for i, x := range a.bindings {
		y := b.bindings[i]
		if x.name != y.name || !sameRetrieval(x.query, y.query) {
			return false
		}
	}
	for i, x := range a.outputs {
		y := b.outputs[i]
		if x.alias != y.alias || !sameExpression(x.value, y.value) {
			return false
		}
	}
	for i, x := range a.links {
		y := b.links[i]
		if x.kind != y.kind || !sameRelation(x.input, y.input) || !sameExpression(x.condition, y.condition) {
			return false
		}
	}
	for i, x := range a.groups {
		if !sameExpression(x, b.groups[i]) {
			return false
		}
	}
	for i, x := range a.order {
		y := b.order[i]
		if x.descending != y.descending || x.nulls != y.nulls || !sameExpression(x.value, y.value) {
			return false
		}
	}
	return true
}

func sameRelation(a, b relation) bool {
	return a.catalog == b.catalog && a.name == b.name && a.alias == b.alias && sameRetrieval(a.query, b.query)
}
