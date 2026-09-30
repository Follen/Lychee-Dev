package memory

import (
	"context"
	"sync"
	"time"
)

// InputRefreshBudget bounds scheduling work within one INPUT observation. It
// retains only four window positions, never records, and must not cross calls.
// Full discovery is outside this quota and retains its original traversal.
type InputRefreshBudget struct {
	execution  chan struct{}
	initialize sync.Once
	mu         sync.Mutex
	windows    [4]uint64
	attempts   int
	bytes      uint64
	reads      int
	elapsed    time.Duration
}

// Source shares the quota across callback searches and their final rereads.
// BeginInputRefresh must succeed before this source can perform any I/O.
func (b *InputRefreshBudget) Source(src Source) Source {
	return &inputRefreshSource{Source: src, budget: b}
}

type inputRefreshSource struct {
	Source
	budget  *InputRefreshBudget
	ctx     context.Context
	cancel  context.CancelFunc
	started time.Time
}

func (s *inputRefreshSource) memorySession() *Session { return sourceSession(s.Source) }

// BeginInputRefresh opens one of at most four 250ms scopes. Repeated searches
// inside the same scope share its deadline, bytes and read quota. final permits
// the single post-traversal search to revisit an earlier window with credit.
func BeginInputRefresh(ctx context.Context, src Source, address uint64, final bool) (context.Context, error) {
	s, ok := src.(*inputRefreshSource)
	if !ok || s.budget == nil {
		return nil, ErrNearbyBudget
	}
	if s.ctx != nil {
		return s.ctx, s.ctx.Err()
	}
	b := s.budget
	b.initialize.Do(func() { b.execution = make(chan struct{}, 1) })
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case b.execution <- struct{}{}:
	}
	b.mu.Lock()
	window := uint64(0)
	if address > nearbyWindowBytes/2 {
		window = (address - nearbyWindowBytes/2) &^ uint64(4095)
	}
	duplicate := false
	for i := 0; i < b.attempts; i++ {
		duplicate = duplicate || b.windows[i] == window
	}
	remaining := time.Second - b.elapsed
	if ctx.Err() != nil || b.attempts >= len(b.windows) || b.bytes >= nearbyMaxBytes || b.reads >= nearbyMaxReads || remaining <= 0 || duplicate && !final {
		b.mu.Unlock()
		<-b.execution
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, ErrNearbyBudget
	}
	b.windows[b.attempts] = window
	b.attempts++
	b.mu.Unlock()
	if remaining > 250*time.Millisecond {
		remaining = 250 * time.Millisecond
	}
	s.started = time.Now()
	s.ctx, s.cancel = context.WithTimeout(ctx, remaining)
	parent := ctx
	if ancestor, ok := ctx.Value(localScopeKey{}).(context.Context); ok {
		parent = ancestor
	}
	s.ctx = localContext(s.ctx, parent)
	return s.ctx, nil
}

// EndInputRefresh must follow the independent final ReadRecord, not precede it.
func EndInputRefresh(src Source) {
	s, ok := src.(*inputRefreshSource)
	if !ok || s.ctx == nil {
		return
	}
	s.budget.mu.Lock()
	s.budget.elapsed += time.Since(s.started)
	s.budget.mu.Unlock()
	s.cancel()
	s.ctx = nil
	<-s.budget.execution
}

func inputRefreshContext(src Source) (context.Context, error) {
	s, ok := src.(*inputRefreshSource)
	if !ok || s.ctx == nil {
		return nil, ErrNearbyBudget
	}
	return s.ctx, s.ctx.Err()
}

func (b *InputRefreshBudget) Available() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.attempts < len(b.windows) && b.bytes < nearbyMaxBytes && b.reads < nearbyMaxReads && b.elapsed < time.Second
}

func (s *inputRefreshSource) readContext(ctx context.Context) (context.Context, error) {
	if s.ctx == nil {
		return nil, ErrNearbyBudget
	}
	if err := s.ctx.Err(); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if deadline, ok := ctx.Deadline(); ok {
		own, _ := s.ctx.Deadline()
		if !deadline.After(own) {
			return ctx, nil
		}
	}
	return s.ctx, nil
}
func (s *inputRefreshSource) Read(ctx context.Context, address uint64, p []byte) (int, error) {
	ctx, err := s.readContext(ctx)
	if err != nil {
		return 0, err
	}
	b := s.budget
	b.mu.Lock()
	if b.reads >= nearbyMaxReads || uint64(len(p)) > nearbyMaxBytes-b.bytes {
		b.mu.Unlock()
		s.cancel()
		return 0, ErrNearbyBudget
	}
	b.reads++
	b.bytes += uint64(len(p))
	b.mu.Unlock()
	return s.Source.Read(ctx, address, p)
}
func (s *inputRefreshSource) Verify(ctx context.Context) error {
	ctx, err := s.readContext(ctx)
	if err != nil {
		return err
	}
	return s.Source.Verify(ctx)
}
func (s *inputRefreshSource) Regions(ctx context.Context) ([]Region, error) {
	ctx, err := s.readContext(ctx)
	if err != nil {
		return nil, err
	}
	return s.Source.Regions(ctx)
}
