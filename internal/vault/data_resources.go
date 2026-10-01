package vault

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"time"
)

// DataResourcePolicy is explicit: measurement drains ordinary data queries and
// then excludes new data queries in this workspace until Close.
type DataResourcePolicy string

type dataResourcePolicyKey struct{}

// WithDataResourcePolicy selects explicit measurement admission for a data
// query without teaching source readers a second coordination path.
func WithDataResourcePolicy(ctx context.Context, policy DataResourcePolicy) context.Context {
	return context.WithValue(ctx, dataResourcePolicyKey{}, policy)
}

const (
	DataOrdinary      DataResourcePolicy = "ordinary"
	DataMeasurement   DataResourcePolicy = "measurement"
	dataResourceSlots                    = 2
)

type DataResources struct {
	once   sync.Once
	leases []*Lease
	err    error
}

func (r *DataResources) Close() error {
	r.once.Do(func() {
		for i := len(r.leases) - 1; i >= 0; i-- {
			r.err = errors.Join(r.err, r.leases[i].Close())
		}
	})
	return r.err
}

// AcquireDataResources uses kernel-owned leases (never an age-based expiry).
// A process death closes its handles; PID reuse cannot steal a live holder.
// Waiting does not open a SQLite connection or hold a metadata transaction.
// Two conservative full-query reservations bound aggregate admission; a
// configured download.workers=1 further serializes ordinary data queries.
func (s *Store) AcquireDataResources(ctx context.Context, policy DataResourcePolicy) (*DataResources, error) {
	if s == nil {
		return nil, ErrWorkspaceFormat
	}
	if requested, ok := ctx.Value(dataResourcePolicyKey{}).(DataResourcePolicy); ok {
		policy = requested
	}
	if policy != DataOrdinary && policy != DataMeasurement {
		return nil, fmt.Errorf("vault: invalid data resource policy %q", policy)
	}
	cfg, err := ReadConfig(ctx, s.Root())
	if err != nil {
		return nil, err
	}
	// Every wait is bounded, even for embedding callers without a deadline.
	bounded, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	scope := filepath.Join(s.Root(), "locks", "data-resources-v1")
	gate, err := acquireDataLease(bounded, scope, "admission")
	if err != nil {
		return nil, err
	}
	out := &DataResources{leases: []*Lease{gate}}
	slots := dataResourceSlots
	if cfg.Download.Workers == 1 {
		slots = 1
	}
	if policy == DataMeasurement {
		for i := 0; i < dataResourceSlots; i++ {
			lease, e := acquireDataLease(bounded, scope, fmt.Sprintf("slot:%d", i))
			if e != nil {
				return nil, errors.Join(e, out.Close())
			}
			out.leases = append(out.leases, lease)
		}
		return out, nil
	}
	for i := 0; i < slots; i++ {
		lease, e := TryAcquireLease(bounded, scope, fmt.Sprintf("slot:%d", i))
		if e == nil {
			if e = gate.Close(); e != nil {
				return nil, errors.Join(e, lease.Close())
			}
			out.leases = []*Lease{lease}
			return out, nil
		}
		if !errors.Is(e, ErrLeaseBusy) {
			return nil, errors.Join(e, out.Close())
		}
	}
	// Keep admission while waiting for slot zero. This prevents new callers
	// overtaking a queued measurement. Slot one can become temporarily idle;
	// avoiding a polling race takes precedence over work-conserving admission.
	lease, err := acquireDataLease(bounded, scope, "slot:0")
	if err != nil {
		return nil, errors.Join(err, out.Close())
	}
	if err = gate.Close(); err != nil {
		return nil, errors.Join(err, lease.Close())
	}
	out.leases = []*Lease{lease}
	return out, nil
}
