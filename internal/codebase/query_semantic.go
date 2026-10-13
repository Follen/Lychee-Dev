package codebase

import (
	"context"
	"strings"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
)

// A CLI batch has one source and API environment. Keep its fixed worktree
// lease and LuaLS worker until the request closes, including on cancellation.
func (b *Browser) analyzeInRequest(ctx context.Context, pin selection.SourcePin, runtime *luals.Runtime, definitions []byte, manifest environment.Manifest, queries []luals.Query) (luals.Analysis, error) {
	owner := ctx.Value(sourceQueryKey{}).(*sourceQuerySession)
	key := strings.Join([]string{b.store.Root(), pin.Repository, pin.Product, pin.ExactCommit, runtime.Identity.SHA256, manifest.InputSHA256, manifest.DefinitionsSHA256}, "\x00")
	session := owner.semantic[key]
	if session == nil {
		lease, err := b.AcquireWorktree(ctx, pin)
		if err != nil {
			return luals.Analysis{}, err
		}
		worker, err := runtime.OpenWorkspaceSession(ctx, lease.Path(), definitions)
		if err != nil {
			lease.Close()
			return luals.Analysis{}, err
		}
		session = &querySemanticSession{worker: worker, lease: lease}
		owner.semantic[key] = session
	}
	return session.worker.AnalyzeBatches(ctx, luals.SplitQueries(queries))
}
