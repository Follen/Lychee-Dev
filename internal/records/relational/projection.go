package relational

// Expressions with subqueries or unresolved output aliases retain full rows.
// Every referenced column in output, filter, join, grouping and order is kept.
func neededColumns(q *retrieval, fields []field) (map[int]bool, bool) {
	selected := map[int]bool{}
	var visit func(*term) bool
	visit = func(n *term) bool {
		if n == nil {
			return true
		}
		if n.query != nil {
			return false
		}
		if n.kind == "star" {
			return false
		}
		if n.kind == "column" {
			i, err := fieldIndex(fields, n.qualifier, n.value)
			if err != nil {
				return false
			}
			selected[i] = true
		}
		for _, a := range n.args {
			if n.kind == "call" && n.value == "COUNT" && a.kind == "star" {
				continue
			}
			if !visit(a) {
				return false
			}
		}
		return true
	}
	for _, p := range q.outputs {
		if !visit(p.value) {
			return nil, false
		}
	}
	for _, j := range q.links {
		if !visit(j.condition) {
			return nil, false
		}
	}
	for _, n := range q.groups {
		if !visit(n) {
			return nil, false
		}
	}
	for _, o := range q.order {
		if !visit(o.value) {
			return nil, false
		}
	}
	return selected, visit(q.filter) && visit(q.having)
}
