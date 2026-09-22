package relational

import (
	"fmt"
	"math/big"
	"strconv"
	"strings"
	"unicode/utf8"
)

func (e *evaluation) function(node *term) (any, error) {
	if node.distinct {
		return nil, ErrUnsupported
	}
	count := len(node.args)
	switch node.value {
	case "LOWER", "UPPER", "LENGTH":
		if count != 1 {
			return nil, ErrType
		}
		value, err := e.value(node.args[0])
		if err != nil || value == nil {
			return value, err
		}
		text, ok := value.(string)
		if !ok {
			return nil, ErrType
		}
		if len(text) > 1<<20 {
			return nil, ErrBudget
		}
		if int64(len(text)) > e.remaining {
			return nil, ErrBudget
		}
		e.remaining -= int64(len(text))
		switch node.value {
		case "LENGTH":
			return int64(utf8.RuneCountInString(text)), nil
		case "LOWER":
			text = strings.ToLower(text)
		case "UPPER":
			text = strings.ToUpper(text)
		}
		if len(text) > 1<<20 {
			return nil, ErrBudget
		}
		return text, nil
	case "COALESCE":
		if count == 0 {
			return nil, ErrType
		}
		for _, arg := range node.args {
			value, err := e.value(arg)
			if err != nil {
				return nil, err
			}
			if value != nil {
				return value, nil
			}
		}
		return nil, nil
	case "NULLIF":
		if count != 2 {
			return nil, ErrType
		}
		left, err := e.value(node.args[0])
		if err != nil {
			return nil, err
		}
		right, err := e.value(node.args[1])
		if err != nil {
			return nil, err
		}
		equal, err := scalarBinary("=", left, right)
		if err != nil {
			return nil, err
		}
		if equal == true {
			return nil, nil
		}
		return left, nil
	default:
		return nil, fmt.Errorf("%w: function %s", ErrUnsupported, node.value)
	}
}

// CAST is explicit and checked. Integer casts truncate finite fractions toward
// zero but never wrap; textual casts preserve the complete uint64 spelling.
func convertScalar(input any, kind string) (any, error) {
	switch kind {
	case "STRING", "TEXT", "VARCHAR", "INT", "INTEGER", "BIGINT", "FLOAT", "DOUBLE", "REAL", "BOOL", "BOOLEAN":
	default:
		return nil, fmt.Errorf("%w: cast %s", ErrUnsupported, kind)
	}
	value, err := canonicalScalar(input)
	if err != nil || value == nil {
		return value, err
	}
	if text, ok := value.(string); ok && len(text) > 1<<20 {
		return nil, ErrBudget
	}
	switch kind {
	case "STRING", "TEXT", "VARCHAR":
		return fmt.Sprint(value), nil
	case "BOOL", "BOOLEAN":
		if v, ok := value.(bool); ok {
			return v, nil
		}
		if v, ok := value.(string); ok {
			result, err := strconv.ParseBool(v)
			if err != nil {
				return nil, ErrType
			}
			return result, nil
		}
		number, err := numberValue(value)
		if err != nil {
			return nil, err
		}
		if number.exact.Sign() == 0 {
			return false, nil
		}
		if number.exact.Cmp(big.NewRat(1, 1)) == 0 {
			return true, nil
		}
		return nil, ErrType
	}
	if text, ok := value.(string); ok {
		value, err = literalNumber(text)
		if err != nil {
			return nil, err
		}
	}
	number, err := numberValue(value)
	if err != nil {
		return nil, err
	}
	switch kind {
	case "INT", "INTEGER", "BIGINT":
		whole := new(big.Int).Quo(number.exact.Num(), number.exact.Denom())
		if !whole.IsInt64() {
			return nil, ErrNumericRange
		}
		return whole.Int64(), nil
	default:
		return materializeNumber(number.exact, false)
	}
}
