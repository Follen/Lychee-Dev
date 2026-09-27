package journal

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/follenfang/lycheedev/internal/vault"
)

// LookupRequest reads an existing idempotency key without acquiring ownership
// or beginning work. It uses exactly the same digest validation as BeginWork.
func (b *Book) LookupRequest(ctx context.Context, intent WorkIntent) (WorkRecord, bool, error) {
	return b.resolveRequest(ctx, intent)
}

// FindRequest reads frozen intent before resolving defaults that may have
// changed since admission. It is not permission to resume: callers must compare
// the proposed digest with LookupRequest before accepting an existing request.
func (b *Book) FindRequest(ctx context.Context, key string) (WorkRecord, bool, error) {
	if key == "" || len(key) > 128 {
		return WorkRecord{}, false, errors.New("journal.invalid_request_identity")
	}
	doc, err := b.metadata.ReadDocument(ctx, "request/"+key)
	if errors.Is(err, vault.ErrMissingRecord) {
		return WorkRecord{}, false, nil
	}
	if err != nil {
		return WorkRecord{}, false, err
	}
	var identity requestIdentity
	if json.Unmarshal(doc.Value, &identity) != nil || identity.OperationID == "" || len(identity.Digest) != 64 {
		return WorkRecord{}, false, errors.New("journal.corrupt_request_identity")
	}
	if _, err := hex.DecodeString(identity.Digest); err != nil {
		return WorkRecord{}, false, errors.New("journal.corrupt_request_identity")
	}
	record, err := b.InspectWork(ctx, identity.OperationID)
	if err != nil {
		return record, false, err
	}
	if record.Intent.RequestKey != key || record.Intent.RequestDigest != identity.Digest {
		return record, false, errors.New("journal.corrupt_request_identity")
	}
	return record, true, nil
}
