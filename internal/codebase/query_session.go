package codebase

import (
	"context"
	"sync"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
)

// A source request owns its verified handles and document memo. Nothing is
// reused across CLI requests; nested Context/Relate calls share this owner.
type sourceQueryKey struct{}
type sourceQuerySession struct {
	mu           sync.Mutex
	caches       map[string]*snapshotCache
	environments map[string]environment.Result
	semantic     map[string]*querySemanticSession
}

type querySemanticSession struct {
	worker *luals.WorkspaceSession
	lease  *WorktreeLease
}

func sourceQueryContext(ctx context.Context) (context.Context, func()) {
	if _, ok := ctx.Value(sourceQueryKey{}).(*sourceQuerySession); ok {
		return ctx, func() {}
	}
	s := &sourceQuerySession{caches: make(map[string]*snapshotCache), environments: make(map[string]environment.Result), semantic: make(map[string]*querySemanticSession)}
	return context.WithValue(ctx, sourceQueryKey{}, s), func() {
		s.mu.Lock()
		defer s.mu.Unlock()
		for key, session := range s.semantic {
			session.worker.Close()
			session.lease.Close()
			delete(s.semantic, key)
		}
		for _, c := range s.caches {
			c.closeResources()
		}
	}
}
