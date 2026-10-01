package records

import "github.com/follenfang/lycheedev/internal/records/resource"

var ErrQueryResourceBudget = resource.ErrBudget

func ensureQueryBudget(q FileQuery) FileQuery {
	if q.budget == nil {
		q.budget = resource.New(resource.DefaultLimits())
	}
	return q
}

// Account retained navigation output conservatively before handing the row to
// a collector. The table layer separately charges all decoded cells as work.
func retainedRowEstimate(row map[string]any) int64 {
	n := int64(64 + len(row)*96)
	var valueSize func(any) int64
	valueSize = func(value any) int64 {
		switch v := value.(type) {
		case string:
			return int64(len(v))
		case []any:
			n := int64(len(v)) * 24
			for _, item := range v {
				n += valueSize(item)
			}
			return n
		case []string:
			n := int64(len(v)) * 24
			for _, item := range v {
				n += int64(len(item))
			}
			return n
		default:
			return 16
		}
	}
	for key, value := range row {
		n += int64(len(key)) + valueSize(value)
	}
	return n
}
