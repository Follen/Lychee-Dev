package relational

// Only a bare equality between a previous input and the new right input is
// indexed. Residual expressions keep nested-loop evaluation/error semantics.
type joinIndex struct {
	left    int
	buckets map[string][][]any
	example any
}

func equalityColumns(condition *term, fields []field, boundary int) (int, int, bool) {
	if condition == nil || condition.kind != "operator" || condition.value != "=" || len(condition.args) != 2 {
		return 0, 0, false
	}
	a, b := condition.args[0], condition.args[1]
	if a.kind != "column" || b.kind != "column" {
		return 0, 0, false
	}
	ai, ae := fieldIndex(fields, a.qualifier, a.value)
	bi, be := fieldIndex(fields, b.qualifier, b.value)
	if ae != nil || be != nil {
		return 0, 0, false
	}
	if ai >= boundary && bi < boundary {
		ai, bi = bi, ai
	}
	return ai, bi - boundary, ai < boundary && bi >= boundary
}

func buildJoinIndex(e *evaluation, condition *term, fields []field, boundary int, rows [][]any) (*joinIndex, error) {
	left, right, ok := equalityColumns(condition, fields, boundary)
	if !ok {
		return nil, nil
	}
	index := &joinIndex{left: left, buckets: map[string][][]any{}}
	// An optional index must leave space for the existing result pipeline.
	// Drop it and restore its charge when the bounded allowance is exhausted.
	allowance, charged := e.retainedBytes/2, int64(0)
	// Heterogeneous keys use the old evaluator, which reports incompatible
	// comparisons only when the actual left stream reaches them.
	for _, row := range rows {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		v := row[right]
		if v == nil {
			continue
		}
		if index.example != nil {
			if _, err := orderScalars(index.example, v); err != nil {
				return nil, nil
			}
		} else {
			index.example = v
		}
	}
	for _, row := range rows {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		v := row[right]
		if v == nil {
			continue
		}
		key, err := equalKey(v)
		if err != nil {
			return nil, err
		}
		cost := int64(32)
		if _, found := index.buckets[key]; !found {
			cost += int64(96 + len(key))
		}
		if cost > allowance-charged {
			e.retainedBytes += charged
			return nil, nil
		}
		e.retainedBytes -= cost
		charged += cost
		index.buckets[key] = append(index.buckets[key], row)
	}
	return index, nil
}

func (index *joinIndex) candidates(left []any) ([][]any, error) {
	v := left[index.left]
	if v == nil || index.example == nil {
		return nil, nil
	}
	if _, err := orderScalars(v, index.example); err != nil {
		return nil, err
	}
	key, err := equalKey(v)
	if err != nil {
		return nil, err
	}
	return index.buckets[key], nil
}
