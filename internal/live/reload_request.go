package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func existingReloadRequest(ctx context.Context, root string, bound RecordedSession, key string) (journal.WorkRecord, error) {
	resource := windowResource(bound.Target)
	digestInput, _ := json.Marshal(struct {
		Resource string `json:"resource"`
		Snapshot string `json:"snapshot"`
		Session  string `json:"session"`
	}{resource, bound.Record.Snapshot, bound.Ready.SessionNonce})
	digest := sha256.Sum256(digestInput)
	request := sha256.Sum256([]byte(resource + "\x00" + key))
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		record, found, err := journal.OpenBook(metadata).LookupRequest(ctx, journal.WorkIntent{RequestKey: "live-" + hex.EncodeToString(request[:]), RequestDigest: hex.EncodeToString(digest[:])})
		if err != nil {
			return record, err
		}
		if found && (record.Intent.Kind != "reload" || record.Intent.Resource != resource || record.Intent.Snapshot != bound.Record.Snapshot) {
			return journal.WorkRecord{}, journal.ErrRequestConflict
		}
		return record, nil
	})
}
