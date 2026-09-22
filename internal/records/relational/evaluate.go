package relational

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

var ErrBinding = errors.New("relational.unresolved_binding")
var ErrUnsupported = errors.New("relational.unsupported_expression")

// evaluation is owned by one execution, never by the immutable Program. Every
// row and nested expression spends from the same remaining work allowance.
type evaluation struct {
	ctx           context.Context
	remaining     int64
	retainedBytes int64
	parameters    map[string]any
	column        func(qualifier, name string) (any, error)
	resolved      map[*term]any
	subquery      func(*retrieval) (Result, error)
	fields        []field
	outer         *rowScope
	planning      bool
	groupAllowed  map[field]bool
}

func (e *evaluation) value(node *term) (any, error) {
	if err := e.ctx.Err(); err != nil {
		return nil, err
	}
	if e.remaining <= 0 {
		return nil, ErrBudget
	}
	e.remaining--
	if node == nil {
		return nil, ErrUnsupported
	}
	if value, found := e.resolved[node]; found {
		return value, nil
	}
	switch node.kind {
	case "outer":
		frame := e.outer
		for depth := 0; depth < node.outerDepth && frame != nil; depth++ {
			frame = frame.parent
		}
		if frame == nil || frame.column == nil {
			return nil, ErrBinding
		}
		value, err := frame.column(node.qualifier, node.value)
		if err != nil {
			return nil, err
		}
		return canonicalScalar(value)
	case "subquery", "exists":
		if e.subquery == nil {
			return nil, ErrUnsupported
		}
		result, err := e.subquery(node.query)
		if err != nil {
			return nil, err
		}
		if node.kind == "exists" {
			return len(result.Rows) > 0, nil
		}
		if len(result.Columns) != 1 || len(result.Rows) > 1 {
			return nil, ErrCardinality
		}
		if len(result.Rows) == 0 {
			return nil, nil
		}
		return result.Rows[0][0], nil
	case "number":
		return literalNumber(node.value)
	case "text":
		return node.value, nil
	case "constant":
		switch node.value {
		case "NULL":
			return nil, nil
		case "TRUE":
			return true, nil
		case "FALSE":
			return false, nil
		}
	case "parameter":
		value, ok := e.parameters[node.value]
		if !ok {
			return nil, fmt.Errorf("%w: parameter %s", ErrBinding, node.value)
		}
		return canonicalScalar(value)
	case "column":
		if e.column == nil {
			return nil, ErrBinding
		}
		value, err := e.column(node.qualifier, node.value)
		if err != nil {
			return nil, err
		}
		return canonicalScalar(value)
	case "unary":
		value, err := e.value(node.args[0])
		if err != nil {
			return nil, err
		}
		return scalarUnary(node.value, value)
	case "operator":
		return e.operator(node)
	case "case":
		return e.choose(node)
	case "call":
		return e.function(node)
	case "cast":
		value, err := e.value(node.args[0])
		if err != nil {
			return nil, err
		}
		return convertScalar(value, node.value)
	}
	return nil, fmt.Errorf("%w: %s", ErrUnsupported, node.kind)
}

func (e *evaluation) choose(node *term) (any, error) {
	at := 0
	var base any
	var err error
	if node.value == "simple" {
		base, err = e.value(node.args[0])
		if err != nil {
			return nil, err
		}
		at++
	}
	for at < len(node.args)-1 {
		condition, err := e.value(node.args[at])
		if err != nil {
			return nil, err
		}
		if node.value == "simple" {
			condition, err = scalarBinary("=", base, condition)
			if err != nil {
				return nil, err
			}
		}
		state, err := booleanState(condition)
		if err != nil {
			return nil, err
		}
		if state == 1 {
			return e.value(node.args[at+1])
		}
		at += 2
	}
	return e.value(node.args[len(node.args)-1])
}

func (e *evaluation) operator(node *term) (any, error) {
	left, err := e.value(node.args[0])
	if err != nil {
		return nil, err
	}
	operation := strings.TrimPrefix(node.value, "NOT ")
	var result any
	switch operation {
	case "IS", "IS NOT":
		return (left == nil) == (operation == "IS"), nil
	case "IN":
		result = false
		if node.query != nil {
			if e.subquery == nil {
				return nil, ErrUnsupported
			}
			rows, err := e.subquery(node.query)
			if err != nil {
				return nil, err
			}
			if len(rows.Columns) != 1 {
				return nil, ErrCardinality
			}
			for _, row := range rows.Rows {
				if err := e.spendMatch(); err != nil {
					return nil, err
				}
				match, err := scalarBinary("=", left, row[0])
				if err != nil {
					return nil, err
				}
				result, err = scalarBinary("OR", result, match)
				if err != nil {
					return nil, err
				}
			}
		}
		for _, candidate := range node.args[1:] {
			right, err := e.value(candidate)
			if err != nil {
				return nil, err
			}
			match, err := scalarBinary("=", left, right)
			if err != nil {
				return nil, err
			}
			result, err = scalarBinary("OR", result, match)
			if err != nil {
				return nil, err
			}
		}
	case "BETWEEN":
		low, err := e.value(node.args[1])
		if err != nil {
			return nil, err
		}
		high, err := e.value(node.args[2])
		if err != nil {
			return nil, err
		}
		lower, err := scalarBinary(">=", left, low)
		if err != nil {
			return nil, err
		}
		upper, err := scalarBinary("<=", left, high)
		if err != nil {
			return nil, err
		}
		result, err = scalarBinary("AND", lower, upper)
		if err != nil {
			return nil, err
		}
	case "LIKE":
		right, err := e.value(node.args[1])
		if err != nil {
			return nil, err
		}
		result, err = e.like(left, right)
		if err != nil {
			return nil, err
		}
	default:
		right, err := e.value(node.args[1])
		if err != nil {
			return nil, err
		}
		return scalarBinary(node.value, left, right)
	}
	if strings.HasPrefix(node.value, "NOT ") {
		return scalarUnary("NOT", result)
	}
	return result, nil
}

// LIKE is case-sensitive Unicode matching: _ consumes one code point, % any
// sequence. No implicit escape convention is borrowed from a SQL driver.
// Dynamic programming avoids recursive wildcard backtracking. Each cell spends
// execution budget, and only two pattern-sized rows are retained.
func (e *evaluation) like(value, pattern any) (any, error) {
	if value == nil || pattern == nil {
		return nil, nil
	}
	text, ok := value.(string)
	if !ok {
		return nil, ErrType
	}
	mask, ok := pattern.(string)
	if !ok {
		return nil, ErrType
	}
	if len(text) > 1<<20 || len(mask) > 1<<20 {
		return nil, ErrBudget
	}
	letters := []rune(mask)
	previous, next := make([]bool, len(letters)+1), make([]bool, len(letters)+1)
	previous[0] = true
	for i, letter := range letters {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		previous[i+1] = previous[i] && letter == '%'
	}
	for _, letter := range text {
		if err := e.spendMatch(); err != nil {
			return nil, err
		}
		next[0] = false
		for i, token := range letters {
			if err := e.spendMatch(); err != nil {
				return nil, err
			}
			switch token {
			case '%':
				next[i+1] = next[i] || previous[i+1]
			case '_':
				next[i+1] = previous[i]
			default:
				next[i+1] = previous[i] && token == letter
			}
		}
		previous, next = next, previous
	}
	return previous[len(letters)], nil
}

func (e *evaluation) spendMatch() error {
	if err := e.ctx.Err(); err != nil {
		return err
	}
	if e.remaining <= 0 {
		return ErrBudget
	}
	e.remaining--
	return nil
}
