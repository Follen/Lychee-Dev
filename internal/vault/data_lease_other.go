//go:build !windows

package vault

import "context"

// Non-Windows development compatibility is outside the product acceptance
// matrix. Preserve its existing lease semantics without changing live locks.
func acquireDataLease(ctx context.Context, scope, resource string) (*Lease, error) {
	return AcquireLease(ctx, scope, resource)
}
