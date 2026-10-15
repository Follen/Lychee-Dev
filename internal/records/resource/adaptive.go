package resource

import "fmt"

const memoryStep int64 = 64 << 20

type adaptiveMemory struct {
	available func() (int64, error)
	ceiling   int64
	shares    int
	growths   int
}

// MemoryBudget describes approved accounting limits, not allocated heap or RSS.
type MemoryBudget struct {
	Mode              string `json:"mode"`
	ConcurrentQueries int    `json:"concurrentQueries,omitempty"`
	CeilingBytes      int64  `json:"ceilingBytes,omitempty"`
	RetainedBytes     int64  `json:"retainedBytes"`
	MetadataBytes     int64  `json:"metadataBytes"`
	PeakScratchBytes  int64  `json:"peakScratchBytes"`
	Growths           int    `json:"growths"`
}

// NewQuery must be called after workspace admission. Windows queries use half
// of currently available physical/commit memory, divided by admitted capacity;
// other development platforms keep the fixed defaults.
func NewQuery(concurrentQueries int) (*Budget, error) {
	return newQueryBudget(concurrentQueries)
}

func newAdaptive(limits Limits, shares int, available func() (int64, error)) (*Budget, error) {
	bytes, err := available()
	if err != nil {
		return nil, fmt.Errorf("%w: memory_status: %w", ErrBudget, err)
	}
	ceiling := bytes / 2 / int64(shares)
	limits.RetainedBytes = min(limits.RetainedBytes, ceiling)
	limits.MetadataBytes = min(limits.MetadataBytes, ceiling)
	return &Budget{limits: limits, memory: &adaptiveMemory{available: available, ceiling: ceiling, shares: shares}}, nil
}

func (b *Budget) MemorySnapshot() MemoryBudget {
	if b == nil {
		return MemoryBudget{}
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	report := MemoryBudget{Mode: "fixed", RetainedBytes: b.limits.RetainedBytes, MetadataBytes: b.limits.MetadataBytes, PeakScratchBytes: b.peakScratch}
	if b.memory != nil {
		report.Mode = "windows_adaptive"
		report.ConcurrentQueries, report.CeilingBytes, report.Growths = b.memory.shares, b.memory.ceiling, b.memory.growths
	}
	return report
}

// growMemory runs under mu after all dimensions pass their hard limits. Check
// fresh headroom only at growth boundaries, never once per decoded cell. The
// initial ceiling stays fixed for this query so repeated growth cannot consume
// all remaining host memory. A later query observes recovered host capacity.
func (b *Budget) growMemory(retained, metadata int64) error {
	if b.memory == nil || retained <= b.limits.RetainedBytes && metadata <= b.limits.MetadataBytes {
		return nil
	}
	available, err := b.memory.available()
	if err != nil {
		return fmt.Errorf("%w: memory_status: %w", ErrBudget, err)
	}
	headroom := available / 2 / int64(b.memory.shares)
	limits := b.limits
	for _, dimension := range []struct {
		name         string
		used, needed int64
		limit        *int64
	}{
		{"retained_bytes", b.used.RetainedBytes + b.scratch, retained, &limits.RetainedBytes},
		{"metadata_bytes", b.used.MetadataBytes, metadata, &limits.MetadataBytes},
	} {
		if dimension.needed <= *dimension.limit {
			continue
		}
		// Only the incremental charge needs fresh memory. Already-accounted
		// allocations have reduced the OS's available memory themselves.
		if dimension.needed-dimension.used > headroom {
			return budgetError(dimension.name, dimension.used, dimension.needed-dimension.used, dimension.used+min(headroom, b.memory.ceiling-dimension.used))
		}
		upper := dimension.used + min(headroom, b.memory.ceiling-dimension.used)
		extra := (memoryStep - dimension.needed%memoryStep) % memoryStep
		*dimension.limit = dimension.needed + min(extra, upper-dimension.needed)
	}
	b.limits = limits
	b.memory.growths++
	return nil
}
