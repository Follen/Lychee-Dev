package codebase

import (
	"context"
	"sync"
)

// A source request owns its verified handles and document memo. Nothing is
// reused across CLI requests; nested Context/Relate calls share this owner.
type sourceQueryKey struct{}
type sourceQuerySession struct {
	mu     sync.Mutex
	caches map[string]*snapshotCache
}

func sourceQueryContext(ctx context.Context) (context.Context, func()) {
	if _, ok := ctx.Value(sourceQueryKey{}).(*sourceQuerySession); ok {
		return ctx, func() {}
	}
	s := &sourceQuerySession{caches: make(map[string]*snapshotCache)}
	return context.WithValue(ctx, sourceQueryKey{}, s), func() {
		s.mu.Lock()
		defer s.mu.Unlock()
		for _, c := range s.caches {
			c.closeResources()
		}
	}
}
