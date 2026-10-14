package journal

import "context"

// LookupRequest reads an existing idempotency key without acquiring ownership
// or beginning work. It uses exactly the same digest validation as BeginWork.
func (b *Book) LookupRequest(ctx context.Context, intent WorkIntent) (WorkRecord, bool, error) {
	return b.resolveRequest(ctx, intent)
}
