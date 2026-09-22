package relational

import (
	"encoding/json"
	"errors"
	"math"
	"math/big"
	"regexp"
	"strconv"
	"strings"
)

var ErrType = errors.New("relational.type_mismatch")
var ErrNumericRange = errors.New("relational.numeric_range")

var decimalLiteral = regexp.MustCompile(`^-?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$`)

// Integer literals never pass through binary floating point. Fractional and
// exponent literals are finite binary64 values; this is not a decimal-money
// dialect. Positive integers may use the full DB2 uint64 range.
func literalNumber(raw string) (any, error) {
	if len(raw) > 1024 {
		return nil, ErrNumericRange
	}
	if !decimalLiteral.MatchString(raw) {
		return nil, ErrType
	}
	if strings.ContainsAny(raw, ".eE") {
		value, err := strconv.ParseFloat(raw, 64)
		if err != nil || math.IsNaN(value) || math.IsInf(value, 0) {
			return nil, ErrNumericRange
		}
		return value, nil
	}
	if value, err := strconv.ParseInt(raw, 10, 64); err == nil {
		return value, nil
	}
	if value, err := strconv.ParseUint(raw, 10, 64); err == nil {
		return value, nil
	}
	return nil, ErrNumericRange
}

// canonicalScalar is also the parameter/provider trust seam: arrays and
// arbitrary Stringer values cannot silently become comparable text or NULL.
func canonicalScalar(value any) (any, error) {
	switch value := value.(type) {
	case nil, bool, string, int64, uint64:
		return value, nil
	case int:
		return int64(value), nil
	case int8:
		return int64(value), nil
	case int16:
		return int64(value), nil
	case int32:
		return int64(value), nil
	case uint:
		return uint64(value), nil
	case uint8:
		return uint64(value), nil
	case uint16:
		return uint64(value), nil
	case uint32:
		return uint64(value), nil
	case float32:
		return canonicalScalar(float64(value))
	case float64:
		if math.IsNaN(value) || math.IsInf(value, 0) {
			return nil, ErrNumericRange
		}
		return value, nil
	case json.Number:
		return literalNumber(string(value))
	default:
		return nil, ErrType
	}
}

type numericValue struct {
	exact    *big.Rat
	integral bool
}

func numberValue(value any) (numericValue, error) {
	switch value := value.(type) {
	case int64:
		return numericValue{new(big.Rat).SetInt64(value), true}, nil
	case uint64:
		return numericValue{new(big.Rat).SetInt(new(big.Int).SetUint64(value)), true}, nil
	case float64:
		if math.IsNaN(value) || math.IsInf(value, 0) {
			return numericValue{}, ErrNumericRange
		}
		return numericValue{new(big.Rat).SetFloat64(value), false}, nil
	default:
		return numericValue{}, ErrType
	}
}
func materializeNumber(value *big.Rat, integer bool) (any, error) {
	if integer {
		if !value.IsInt() {
			return nil, ErrType
		}
		whole := value.Num()
		if whole.IsInt64() {
			return whole.Int64(), nil
		}
		if whole.Sign() >= 0 && whole.IsUint64() {
			return whole.Uint64(), nil
		}
		return nil, ErrNumericRange
	}
	real, _ := value.Float64()
	if math.IsInf(real, 0) || math.IsNaN(real) {
		return nil, ErrNumericRange
	}
	return real, nil
}

// orderScalars is a non-NULL typed comparator. ORDER BY supplies its explicit
// NULL policy separately; predicates must return unknown for a NULL operand.
// Rat conversion of finite floats compares their exact binary values to integers
// instead of rounding a uint64 operand to that float first.
func orderScalars(left, right any) (int, error) {
	a, err := canonicalScalar(left)
	if err != nil {
		return 0, err
	}
	b, err := canonicalScalar(right)
	if err != nil {
		return 0, err
	}
	switch value := a.(type) {
	case string:
		other, ok := b.(string)
		if !ok {
			return 0, ErrType
		}
		return strings.Compare(value, other), nil
	case bool:
		other, ok := b.(bool)
		if !ok {
			return 0, ErrType
		}
		if value == other {
			return 0, nil
		}
		if value {
			return 1, nil
		}
		return -1, nil
	case nil:
		return 0, ErrType
	}
	x, err := numberValue(a)
	if err != nil {
		return 0, err
	}
	y, err := numberValue(b)
	if err != nil {
		return 0, err
	}
	return x.exact.Cmp(y.exact), nil
}

// booleanState uses -1 for SQL unknown, 0 for false, 1 for true.
func booleanState(value any) (int, error) {
	if value == nil {
		return -1, nil
	}
	if boolean, ok := value.(bool); ok {
		if boolean {
			return 1, nil
		}
		return 0, nil
	}
	return 0, ErrType
}
func booleanValue(state int) any {
	if state < 0 {
		return nil
	}
	return state == 1
}

func scalarUnary(operation string, value any) (any, error) {
	input, err := canonicalScalar(value)
	if err != nil {
		return nil, err
	}
	if operation == "NOT" {
		state, err := booleanState(input)
		if err != nil {
			return nil, err
		}
		if state < 0 {
			return nil, nil
		}
		return state == 0, nil
	}
	if operation != "+" && operation != "-" {
		return nil, ErrType
	}
	if input == nil {
		return nil, nil
	}
	number, err := numberValue(input)
	if err != nil {
		return nil, err
	}
	if operation == "-" {
		number.exact.Neg(number.exact)
	}
	return materializeNumber(number.exact, number.integral)
}

func scalarBinary(operation string, left, right any) (any, error) {
	switch operation {
	case "AND", "OR", "=", "!=", "<>", "<", "<=", ">", ">=", "||", "+", "-", "*", "/", "%":
	default:
		return nil, ErrType
	}
	a, err := canonicalScalar(left)
	if err != nil {
		return nil, err
	}
	b, err := canonicalScalar(right)
	if err != nil {
		return nil, err
	}
	if operation == "AND" || operation == "OR" {
		x, err := booleanState(a)
		if err != nil {
			return nil, err
		}
		y, err := booleanState(b)
		if err != nil {
			return nil, err
		}
		if operation == "AND" {
			if x == 0 || y == 0 {
				return false, nil
			}
			if x < 0 || y < 0 {
				return nil, nil
			}
			return true, nil
		}
		if x == 1 || y == 1 {
			return true, nil
		}
		if x < 0 || y < 0 {
			return nil, nil
		}
		return false, nil
	}
	if a == nil || b == nil {
		return nil, nil
	}
	switch operation {
	case "=", "!=", "<>", "<", "<=", ">", ">=":
		order, err := orderScalars(a, b)
		if err != nil {
			return nil, err
		}
		switch operation {
		case "=":
			return order == 0, nil
		case "!=", "<>":
			return order != 0, nil
		case "<":
			return order < 0, nil
		case "<=":
			return order <= 0, nil
		case ">":
			return order > 0, nil
		default:
			return order >= 0, nil
		}
	case "||":
		x, ok := a.(string)
		if !ok {
			return nil, ErrType
		}
		y, ok := b.(string)
		if !ok {
			return nil, ErrType
		}
		if len(x) > 1<<20-len(y) {
			return nil, ErrBudget
		}
		return x + y, nil
	case "+", "-", "*", "/", "%":
		x, err := numberValue(a)
		if err != nil {
			return nil, err
		}
		y, err := numberValue(b)
		if err != nil {
			return nil, err
		}
		result := new(big.Rat)
		integral := x.integral && y.integral
		switch operation {
		case "+":
			result.Add(x.exact, y.exact)
		case "-":
			result.Sub(x.exact, y.exact)
		case "*":
			result.Mul(x.exact, y.exact)
		case "/":
			if y.exact.Sign() == 0 {
				return nil, nil
			}
			result.Quo(x.exact, y.exact)
			integral = false
		case "%":
			if y.exact.Sign() == 0 {
				return nil, nil
			}
			quotient := new(big.Rat).Quo(x.exact, y.exact)
			whole := new(big.Int).Quo(quotient.Num(), quotient.Denom())
			result.Sub(x.exact, new(big.Rat).Mul(new(big.Rat).SetInt(whole), y.exact))
		}
		return materializeNumber(result, integral)
	default:
		return nil, ErrType
	}
}
