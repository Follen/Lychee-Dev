package relational

import "sort"

type PlanStep struct {
	ID        int    `json:"id"`
	Parent    int    `json:"parent"`
	Operation string `json:"operation"`
	Detail    string `json:"detail,omitempty"`
}
type ScanMeasurement struct {
	Table TableUse `json:"table"`
	Calls int64    `json:"calls"`
	Rows  int64    `json:"rows"`
}
type ExecutionMeasurement struct {
	Scans        []ScanMeasurement `json:"scans"`
	WorkUnits    int64             `json:"workUnits"`
	ChargedBytes int64             `json:"chargedBytes"`
	OutputRows   int               `json:"outputRows"`
}
type Explanation struct {
	OutputColumns []string              `json:"outputColumns"`
	Steps         []PlanStep            `json:"steps"`
	Measured      *ExecutionMeasurement `json:"measured,omitempty"`
}

// This is an execution-shape description, not a cost-based optimizer estimate.
// ChargedBytes is cumulative budget accounting, not process RSS or peak memory.
func (p *Program) explanation(e *evaluation) (*Explanation, error) {
	plan := &Explanation{}
	add := func(parent int, operation, detail string) (int, error) {
		if err := e.spendMatch(); err != nil {
			return 0, err
		}
		cost := int64(64 + len(detail) + len(operation))
		if cost > e.retainedBytes {
			return 0, ErrBudget
		}
		e.retainedBytes -= cost
		id := len(plan.Steps) + 1
		plan.Steps = append(plan.Steps, PlanStep{ID: id, Parent: parent, Operation: operation, Detail: detail})
		return id, nil
	}
	var query func(*retrieval, int) error
	var expression func(*term, int) error
	expression = func(node *term, parent int) error {
		if node == nil {
			return nil
		}
		if err := e.spendMatch(); err != nil {
			return err
		}
		if node.query != nil {
			id, err := add(parent, "expression-subquery", node.kind)
			if err != nil {
				return err
			}
			if err := query(node.query, id); err != nil {
				return err
			}
		}
		for _, arg := range node.args {
			if err := expression(arg, parent); err != nil {
				return err
			}
		}
		return nil
	}
	query = func(q *retrieval, parent int) error {
		id, err := add(parent, "select", "")
		if err != nil {
			return err
		}
		for _, binding := range q.bindings {
			kind := "materialized-cte"
			if q.recursive && recursiveReferences(binding.query, binding.name) > 0 {
				kind = "recursive-delta"
			}
			child, err := add(id, kind, binding.name)
			if err != nil {
				return err
			}
			if err := query(binding.query, child); err != nil {
				return err
			}
		}
		input := func(r relation, parent int) error {
			if r.query != nil {
				return query(r.query, parent)
			}
			name := r.name
			if r.catalog != "" {
				name = r.catalog + "." + name
			}
			_, err := add(parent, "input", name)
			return err
		}
		if err := input(q.input, id); err != nil {
			return err
		}
		for _, join := range q.links {
			child, err := add(id, "nested-loop-join", join.kind+"; materialize right")
			if err != nil {
				return err
			}
			if err := input(join.input, child); err != nil {
				return err
			}
			if err := expression(join.condition, child); err != nil {
				return err
			}
		}
		if q.filter != nil {
			child, err := add(id, "filter", "")
			if err != nil {
				return err
			}
			if err := expression(q.filter, child); err != nil {
				return err
			}
		}
		grouped := len(q.groups) > 0 || q.having != nil
		for _, out := range q.outputs {
			grouped = grouped || containsAggregate(out.value)
		}
		for _, order := range q.order {
			grouped = grouped || containsAggregate(order.value)
		}
		if grouped {
			if _, err := add(id, "group", "hash keys; streaming accumulators"); err != nil {
				return err
			}
		}
		if err := expression(q.having, id); err != nil {
			return err
		}
		for _, out := range q.outputs {
			if err := expression(out.value, id); err != nil {
				return err
			}
		}
		if q.distinct {
			if _, err := add(id, "distinct", "typed value keys"); err != nil {
				return err
			}
		}
		if len(q.order) > 0 {
			if _, err := add(id, "order", "stable merge sort"); err != nil {
				return err
			}
		}
		for _, order := range q.order {
			if err := expression(order.value, id); err != nil {
				return err
			}
		}
		if q.limit != nil || q.offset != nil {
			if _, err := add(id, "slice", "offset then limit"); err != nil {
				return err
			}
		}
		if q.union != nil {
			child, err := add(id, "union-all", "tail order/slice apply to compound result")
			if err != nil {
				return err
			}
			return query(q.union, child)
		}
		return nil
	}
	if err := query(p.root, 0); err != nil {
		return nil, err
	}
	return plan, nil
}

func measureExecution(scans map[TableUse]*ScanMeasurement, work, bytes int64, rows int) *ExecutionMeasurement {
	result := &ExecutionMeasurement{Scans: []ScanMeasurement{}, WorkUnits: work, ChargedBytes: bytes, OutputRows: rows}
	for _, scan := range scans {
		result.Scans = append(result.Scans, *scan)
	}
	sort.Slice(result.Scans, func(i, j int) bool {
		a, b := result.Scans[i].Table, result.Scans[j].Table
		if a.Catalog != b.Catalog {
			return a.Catalog < b.Catalog
		}
		return a.Name < b.Name
	})
	return result
}
