package relational

import (
	"strconv"
	"strings"
)

func rowKey(e *evaluation, values []any) (string, error) {
	var key strings.Builder
	for _, value := range values {
		if err := e.spendMatch(); err != nil {
			return "", err
		}
		part, err := equalKey(value)
		if err != nil {
			return "", err
		}
		if int64(len(part)) > e.remaining {
			return "", ErrBudget
		}
		e.remaining -= int64(len(part))
		if int64(key.Len()+len(part)+32) > e.retainedBytes {
			return "", ErrBudget
		}
		key.WriteString(strconv.Itoa(len(part)))
		key.WriteByte(':')
		key.WriteString(part)
	}
	return key.String(), nil
}

func resultCount(e *evaluation, node *term, fallback uint64) (uint64, error) {
	if node == nil {
		return fallback, nil
	}
	value, err := e.value(node)
	if err != nil {
		return 0, err
	}
	switch value := value.(type) {
	case int64:
		if value >= 0 {
			return uint64(value), nil
		}
	case uint64:
		return value, nil
	}
	return 0, ErrType
}

func compareRows(e *evaluation, order []ordering, a, b groupOutput) (int, error) {
	for i, term := range order {
		if err := e.spendMatch(); err != nil {
			return 0, err
		}
		left, right := a.order[i], b.order[i]
		if left == nil && right == nil {
			continue
		}
		if left == nil || right == nil {
			first := !term.descending
			if term.nulls != "" {
				first = term.nulls == "first"
			}
			if (left == nil) == first {
				return -1, nil
			}
			return 1, nil
		}
		result, err := orderScalars(left, right)
		if err != nil {
			return 0, err
		}
		if result != 0 {
			if term.descending {
				result = -result
			}
			return result, nil
		}
	}
	return 0, nil
}

// finishResults performs DISTINCT, stable ordering, then OFFSET/LIMIT. It owns
// slice storage so failures cannot reorder the caller's input. Merge comparison
// errors and cancellation propagate instead of violating sort's comparator law.
func finishResults(e *evaluation, query *retrieval, input []groupOutput) ([]groupOutput, error) {
	limit, err := resultCount(e, query.limit, ^uint64(0))
	if err != nil {
		return nil, err
	}
	offset, err := resultCount(e, query.offset, 0)
	if err != nil {
		return nil, err
	}
	cost := int64(len(input)) * 96
	if cost > e.retainedBytes {
		return nil, ErrBudget
	}
	e.retainedBytes -= cost
	rows := make([]groupOutput, 0, len(input))
	seen := make(map[string]struct{})
	for _, row := range input {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		if len(row.order) != len(query.order) {
			return nil, ErrBinding
		}
		if query.distinct {
			key, err := rowKey(e, row.values)
			if err != nil {
				return nil, err
			}
			if _, found := seen[key]; found {
				continue
			}
			cost := int64(len(key)) + 64
			if cost > e.retainedBytes {
				return nil, ErrBudget
			}
			e.retainedBytes -= cost
			seen[key] = struct{}{}
		}
		rows = append(rows, row)
	}
	if len(query.order) > 0 {
		buffer := make([]groupOutput, len(rows))
		for width := 1; width < len(rows); width *= 2 {
			for start := 0; start < len(rows); start += 2 * width {
				mid, end := min(start+width, len(rows)), min(start+2*width, len(rows))
				i, j := start, mid
				for at := start; at < end; at++ {
					if err := e.spendMatch(); err != nil {
						return nil, err
					}
					left := j == end
					if i < mid && j < end {
						order, err := compareRows(e, query.order, rows[i], rows[j])
						if err != nil {
							return nil, err
						}
						left = order <= 0
					}
					if i < mid && left {
						buffer[at] = rows[i]
						i++
					} else {
						buffer[at] = rows[j]
						j++
					}
				}
			}
			rows, buffer = buffer, rows
		}
	}
	if offset >= uint64(len(rows)) {
		return []groupOutput{}, nil
	}
	end := uint64(len(rows))
	if limit < end-offset {
		end = offset + limit
	}
	return rows[int(offset):int(end)], nil
}
