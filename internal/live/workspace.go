package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// Serializes phases of one operation across processes. Identity validation and
// intent CAS remain mandatory; this is not the machine-wide game-input lease.
func withOperation[T any](ctx context.Context, root, operationID string, run func(*vault.Store, *vault.Metadata) (T, error)) (T, error) {
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (result T, err error) {
		lease, err := vault.AcquireLease(ctx, filepath.Join(store.Root(), "locks"), "operation:"+operationID)
		if err != nil {
			return result, err
		}
		defer func() { err = errors.Join(err, lease.Close()) }()
		return run(store, metadata)
	})
}
