package relational

type rankedRow struct {
	row     groupOutput
	ordinal uint64
	charged int64
}
type topRows struct {
	rows    []rankedRow
	size    uint64
	next    uint64
	order   []ordering
	example any
}

func newTopRows(e *evaluation, q *retrieval, grouped bool) (*topRows, error) {
	if grouped || q.distinct || q.limit == nil || len(q.order) > 1 {
		return nil, nil
	}
	limit, err := resultCount(e, q.limit, 0)
	if err != nil {
		return nil, err
	}
	offset, err := resultCount(e, q.offset, 0)
	if err != nil {
		return nil, err
	}
	if offset > ^uint64(0)-limit || offset+limit > 1000000 {
		return nil, nil
	}
	return &topRows{size: offset + limit, order: q.order}, nil
}

func outputCost(row groupOutput) int64 {
	cost := int64(128 + 32*(len(row.values)+len(row.order)))
	for _, values := range [][]any{row.values, row.order} {
		for _, v := range values {
			if text, ok := v.(string); ok {
				cost += int64(len(text))
			}
		}
	}
	return cost
}

func (t *topRows) worse(e *evaluation, a, b rankedRow) (bool, error) {
	order, err := compareRows(e, t.order, a.row, b.row)
	if err != nil {
		return false, err
	}
	return order > 0 || order == 0 && a.ordinal > b.ordinal, nil
}

func (t *topRows) add(e *evaluation, row groupOutput) error {
	candidate := rankedRow{row: row, ordinal: t.next}
	t.next++
	if len(row.order) == 1 && row.order[0] != nil {
		if t.example != nil {
			if _, err := orderScalars(t.example, row.order[0]); err != nil {
				return err
			}
		} else {
			t.example = row.order[0]
		}
	}
	if t.size == 0 {
		return nil
	}
	cost := outputCost(row)
	if uint64(len(t.rows)) < t.size {
		if cost > e.retainedBytes {
			return ErrBudget
		}
		e.retainedBytes -= cost
		candidate.charged = cost
		t.rows = append(t.rows, candidate)
		for i := len(t.rows) - 1; i > 0; {
			p := (i - 1) / 2
			worse, err := t.worse(e, t.rows[i], t.rows[p])
			if err != nil {
				return err
			}
			if !worse {
				break
			}
			t.rows[i], t.rows[p] = t.rows[p], t.rows[i]
			i = p
		}
		return nil
	}
	replace, err := t.worse(e, t.rows[0], candidate)
	if err != nil {
		return err
	}
	if !replace {
		return nil
	}
	// Reuse the retained slot; charge only growth, never cumulative input rows.
	candidate.charged = max(cost, t.rows[0].charged)
	growth := candidate.charged - t.rows[0].charged
	if growth > e.retainedBytes {
		return ErrBudget
	}
	e.retainedBytes -= growth
	t.rows[0] = candidate
	for i := 0; ; {
		child := i*2 + 1
		if child >= len(t.rows) {
			break
		}
		if child+1 < len(t.rows) {
			worse, err := t.worse(e, t.rows[child+1], t.rows[child])
			if err != nil {
				return err
			}
			if worse {
				child++
			}
		}
		worse, err := t.worse(e, t.rows[child], t.rows[i])
		if err != nil {
			return err
		}
		if !worse {
			break
		}
		t.rows[i], t.rows[child] = t.rows[child], t.rows[i]
		i = child
	}
	return nil
}

func (t *topRows) finish(e *evaluation) ([]groupOutput, error) {
	// Heap extraction gives reverse total order, including stable input ordinals.
	out := make([]groupOutput, len(t.rows))
	for end := len(t.rows) - 1; end >= 0; end-- {
		out[end] = t.rows[0].row
		t.rows[0] = t.rows[len(t.rows)-1]
		t.rows = t.rows[:len(t.rows)-1]
		for i := 0; ; {
			child := i*2 + 1
			if child >= len(t.rows) {
				break
			}
			if child+1 < len(t.rows) {
				worse, err := t.worse(e, t.rows[child+1], t.rows[child])
				if err != nil {
					return nil, err
				}
				if worse {
					child++
				}
			}
			worse, err := t.worse(e, t.rows[child], t.rows[i])
			if err != nil {
				return nil, err
			}
			if !worse {
				break
			}
			t.rows[i], t.rows[child] = t.rows[child], t.rows[i]
			i = child
		}
	}
	return out, nil
}
