package relational

import (
	"math"
	"math/big"
)

// aggregate keeps streaming state, not the group's input rows. DISTINCT storage
// is charged to the owning execution; integer sums round only at finalization.
type aggregate struct {
	name         string
	star         bool
	count        int64
	sum          big.Rat
	integer      bool
	extreme      any
	extremeBytes int64
	seen         map[string]struct{}
}

func newAggregate(node *term) (*aggregate, error) {
	switch node.value {
	case "COUNT", "MIN", "MAX", "SUM", "AVG":
	default:
		return nil, ErrUnsupported
	}
	if len(node.args) != 1 {
		return nil, ErrType
	}
	star := node.args[0].kind == "star"
	if star && (node.value != "COUNT" || node.distinct || node.args[0].qualifier != "") {
		return nil, ErrType
	}
	a := &aggregate{name: node.value, star: star, integer: true}
	if node.distinct {
		a.seen = make(map[string]struct{})
	}
	return a, nil
}

// equalKey shares numeric equality with comparisons: 1, uint64(1), and 1.0
// have one key, while large rounded floats never collide with nearby integers.
func equalKey(input any) (string, error) {
	value, err := canonicalScalar(input)
	if err != nil {
		return "", err
	}
	switch value := value.(type) {
	case nil:
		return "z", nil
	case string:
		if len(value) > 1<<20 {
			return "", ErrBudget
		}
		return "s" + value, nil
	case bool:
		if value {
			return "b1", nil
		}
		return "b0", nil
	default:
		number, err := numberValue(value)
		if err != nil {
			return "", err
		}
		return "n" + number.exact.RatString(), nil
	}
}

func (a *aggregate) add(e *evaluation, input any) error {
	if err := e.spendMatch(); err != nil {
		return err
	}
	value, err := canonicalScalar(input)
	if err != nil {
		return err
	}
	if !a.star && value == nil {
		return nil
	}
	if text, ok := value.(string); ok && len(text) > 1<<20 {
		return ErrBudget
	}
	if a.seen != nil {
		key, err := equalKey(value)
		if err != nil {
			return err
		}
		if _, found := a.seen[key]; found {
			return nil
		}
		// Include per-entry overhead rather than counting only payload bytes.
		cost := int64(len(key)) + 64
		if e.retainedBytes < cost {
			return ErrBudget
		}
		e.retainedBytes -= cost
		a.seen[key] = struct{}{}
	}
	if a.count == math.MaxInt64 {
		return ErrNumericRange
	}
	switch a.name {
	case "MIN", "MAX":
		if a.count == 0 {
			if err := a.retainExtreme(e, value); err != nil {
				return err
			}
		} else {
			order, err := orderScalars(value, a.extreme)
			if err != nil {
				return err
			}
			if (a.name == "MIN" && order < 0) || (a.name == "MAX" && order > 0) {
				if err := a.retainExtreme(e, value); err != nil {
					return err
				}
			}
		}
	case "SUM", "AVG":
		number, err := numberValue(value)
		if err != nil {
			return err
		}
		a.sum.Add(&a.sum, number.exact)
		a.integer = a.integer && number.integral
	}
	a.count++
	return nil
}

func (a *aggregate) result() (any, error) {
	if a.name == "COUNT" {
		return a.count, nil
	}
	if a.count == 0 {
		return nil, nil
	}
	switch a.name {
	case "MIN", "MAX":
		return a.extreme, nil
	case "SUM":
		return materializeNumber(&a.sum, a.integer)
	case "AVG":
		return materializeNumber(new(big.Rat).Quo(&a.sum, new(big.Rat).SetInt64(a.count)), false)
	default:
		return nil, ErrUnsupported
	}
}

func (a *aggregate) retainExtreme(e *evaluation, value any) error {
	cost := int64(0)
	if text, ok := value.(string); ok {
		cost = int64(len(text))
	}
	delta := cost - a.extremeBytes
	if delta > e.retainedBytes {
		return ErrBudget
	}
	e.retainedBytes -= delta
	a.extremeBytes, a.extreme = cost, value
	return nil
}
