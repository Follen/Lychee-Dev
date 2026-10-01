// Package resource accounts for one data query across all prepared sources.
// The byte charges are conservative allocation estimates, not a process RSS cap.
package resource

import (
	"errors"
	"fmt"
	"sync"
)

var ErrBudget = errors.New("records.query_resource_budget")

type Cost struct {
	RetainedBytes   int64 `json:"retainedBytes"`
	MetadataBytes   int64 `json:"metadataBytes"`
	DecodeWork      int64 `json:"decodeWork"`
	NetworkRequests int64 `json:"networkRequests"`
	NetworkBytes    int64 `json:"networkBytes"`
}
type Limits = Cost
type Usage = Cost

func DefaultLimits() Limits {
	return Limits{RetainedBytes: 512 << 20, MetadataBytes: 128 << 20, DecodeWork: 100000000, NetworkRequests: 4096, NetworkBytes: 1 << 30}
}

type Budget struct {
	mu                   sync.Mutex
	limits               Limits
	used                 Usage
	scratch, peakScratch int64
}

func New(limits Limits) *Budget { return &Budget{limits: limits} }
func (b *Budget) Snapshot() Usage {
	if b == nil {
		return Usage{}
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.used
}
func (b *Budget) RemainingRetained() int64 {
	if b == nil {
		return 512 << 20
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.limits.RetainedBytes - b.used.RetainedBytes - b.scratch
}

func (b *Budget) RemainingNetworkBytes() int64 {
	if b == nil {
		return 1 << 30
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.limits.NetworkBytes - b.used.NetworkBytes
}

// Charge is atomic: a rejected multi-dimensional charge changes no counters.
// Retained and metadata charges reserve conservatively for the query lifetime;
// work and network charges include failed attempts and cannot be refunded.
func (b *Budget) Charge(c Cost) error {
	if b == nil {
		return nil
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, d := range []struct {
		name                string
		current, add, limit int64
	}{
		{"retained_bytes", b.used.RetainedBytes + b.scratch, c.RetainedBytes, b.limits.RetainedBytes},
		{"metadata_bytes", b.used.MetadataBytes, c.MetadataBytes, b.limits.MetadataBytes},
		{"decode_work", b.used.DecodeWork, c.DecodeWork, b.limits.DecodeWork},
		{"network_requests", b.used.NetworkRequests, c.NetworkRequests, b.limits.NetworkRequests},
		{"network_bytes", b.used.NetworkBytes, c.NetworkBytes, b.limits.NetworkBytes},
	} {
		if d.add < 0 || d.current > d.limit || d.add > d.limit-d.current {
			return fmt.Errorf("%w: %s", ErrBudget, d.name)
		}
	}
	b.used.RetainedBytes += c.RetainedBytes
	b.used.MetadataBytes += c.MetadataBytes
	b.used.DecodeWork += c.DecodeWork
	b.used.NetworkRequests += c.NetworkRequests
	b.used.NetworkBytes += c.NetworkBytes
	return nil
}

// ReserveScratch charges live decoder buffers against the same retained cap,
// then releases the reservation when that bounded step finishes. This accounts
// logical live buffers rather than claiming Go's GC promptly reduces RSS.
func (b *Budget) ReserveScratch(bytes int64) (func(), error) {
	if b == nil {
		return func() {}, nil
	}
	b.mu.Lock()
	if bytes < 0 || bytes > b.limits.RetainedBytes-b.used.RetainedBytes-b.scratch {
		b.mu.Unlock()
		return nil, fmt.Errorf("%w: scratch_bytes", ErrBudget)
	}
	b.scratch += bytes
	b.peakScratch = max(b.peakScratch, b.scratch)
	b.mu.Unlock()
	var once sync.Once
	return func() { once.Do(func() { b.mu.Lock(); b.scratch -= bytes; b.mu.Unlock() }) }, nil
}

func (b *Budget) PeakScratchBytes() int64 {
	if b == nil {
		return 0
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.peakScratch
}
