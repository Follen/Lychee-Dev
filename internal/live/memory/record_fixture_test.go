package memory

import (
	"context"
	"errors"
	"sync"
)

type fakeSource struct {
	mu        sync.Mutex
	data      []byte
	holes     []Range
	transient bool
	calls     int
}

func (s *fakeSource) Verify(context.Context) error { return nil }
func (s *fakeSource) Read(ctx context.Context, address uint64, b []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.calls++
	if ctx.Err() != nil {
		return 0, ctx.Err()
	}
	if s.transient && len(b) > 4096 {
		return 0, errors.New("transient block failure")
	}
	end := address + uint64(len(b))
	if end > uint64(len(s.data)) {
		return 0, errors.New("outside")
	}
	for _, h := range s.holes {
		if address < h.End && h.Start < end {
			return 0, errors.New("hole")
		}
	}
	copy(b, s.data[address:end])
	return len(b), nil
}
func source(size int) *fakeSource {
	return &fakeSource{data: make([]byte, size)}
}
