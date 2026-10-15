package records

import (
	"context"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrQueryResourceBudget = resource.ErrBudget

// Admission comes before the memory snapshot: a queued query can wait for up
// to 30 seconds. Every nested reader reuses both admission and the same budget.
func acquireQueryResources(ctx context.Context, store *vault.Store, q FileQuery) (FileQuery, *vault.DataResources, error) {
	if q.admitted {
		return q, nil, nil
	}
	admission, err := store.AcquireDataResources(ctx, vault.DataOrdinary)
	if err != nil {
		return q, nil, err
	}
	if q.budget == nil {
		q.budget, err = resource.NewQuery(admission.ConcurrentQueries)
		if err != nil {
			admission.Close()
			return q, nil, err
		}
	}
	q.admitted, q.downloadWorkers = true, admission.DownloadWorkers
	return q, admission, nil
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
