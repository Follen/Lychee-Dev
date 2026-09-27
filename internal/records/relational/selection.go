package relational

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
