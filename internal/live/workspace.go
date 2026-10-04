package live

import (
	"context"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func InspectOperation(ctx context.Context, root, id string) (journal.WorkRecord, error) {
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (journal.WorkRecord, error) {
		return journal.OpenBook(m).InspectWork(ctx, id)
	})
}
