package relational

import "errors"

func bareEquality(node *term) bool {
	return node != nil && node.kind == "operator" && node.value == "=" && len(node.args) == 2
}

func equalitySelection(e *evaluation, node *term, fields []field) (string, any, bool, error) {
	if !bareEquality(node) {
		return "", nil, false, nil
	}
	column, value := node.args[0], node.args[1]
	if column.kind != "column" {
		column, value = value, column
	}
	if column.kind != "column" {
		return "", nil, false, nil
	}
	switch value.kind {
	case "number", "text", "constant", "parameter":
	default:
		return "", nil, false, nil
	}
	index, err := fieldIndex(fields, column.qualifier, column.value)
	if err != nil {
		return "", nil, false, err
	}
	scalar, err := e.value(value)
	return fields[index].name, scalar, true, err
}

var errSelectionBuffer = errors.New("relational.selection_buffer_limit")

// A bare numeric IN list can use the source's existing equality lookup. Keep
// the original predicate as the final filter, including its NULL semantics.
// Mixed types and expressions retain the scan's evaluation/error behavior.
func inSelection(e *evaluation, node *term, fields []field, source Source, yield func([]any) error) (bool, error) {
	if node == nil || node.kind != "operator" || node.value != "IN" || node.query != nil || len(node.args) < 2 || node.args[0].kind != "column" {
		return false, nil
	}
	column := node.args[0]
	index, err := fieldIndex(fields, column.qualifier, column.value)
	if err != nil {
		return true, err
	}
	allowance, charged := e.retainedBytes/2, int64(0)
	defer func() { e.retainedBytes += charged }()
	reserve := func(cost int64) bool {
		if cost > allowance-charged {
			return false
		}
		e.retainedBytes -= cost
		charged += cost
		return true
	}
	values, seen := []any{}, map[string]bool{}
	for _, candidate := range node.args[1:] {
		switch candidate.kind {
		case "number", "constant", "parameter":
		case "unary":
			if (candidate.value != "+" && candidate.value != "-") || len(candidate.args) != 1 || candidate.args[0].kind != "number" {
				return false, nil
			}
		default:
			return false, nil
		}
		value, err := e.value(candidate)
		if err != nil {
			if errors.Is(err, ErrBudget) || e.ctx.Err() != nil {
				return true, err
			}
			// An invalid literal still fails only if the scan reaches a row.
			return false, nil
		}
		switch value.(type) {
		case nil, int64, uint64, float64:
		default:
			return false, nil
		}
		key, err := equalKey(value)
		if err != nil {
			return true, err
		}
		if !seen[key] {
			if !reserve(int64(96 + len(key))) {
				return false, nil
			}
			seen[key] = true
			values = append(values, value)
		}
	}
	// Providers can decline individual values. Do not emit any candidates until
	// all lookups are handled, so fallback cannot duplicate earlier matches.
	var rows [][]any
	for _, value := range values {
		handled, err := source.Equal(e.ctx, fields[index].name, value, func(row []any) error {
			if err := e.spendMatch(); err != nil {
				return err
			}
			if len(row) != len(fields) {
				return ErrBinding
			}
			cost := int64(64 + 32*len(row))
			for _, value := range row {
				if text, ok := value.(string); ok {
					cost += int64(len(text))
				}
			}
			if !reserve(cost) {
				return errSelectionBuffer
			}
			rows = append(rows, append([]any(nil), row...))
			return nil
		})
		if errors.Is(err, errSelectionBuffer) {
			return false, nil
		}
		if err != nil || !handled {
			return handled || err != nil, err
		}
	}
	for _, row := range rows {
		if err := yield(row); err != nil {
			return true, err
		}
	}
	return true, nil
}
