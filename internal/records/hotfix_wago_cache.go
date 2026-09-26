// SPDX-License-Identifier: MIT
// Page cache receipts adapted from wowdata's wago cache; see LICENSE.
package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// Workspace-cached per-page receipts replace the legacy file pair: one receipt
// document per page identity plus the verified response bytes as a content
// addressed blob. A corrupt or mismatched receipt fails explicitly
// (records.hotfix_cache_corrupt) and is never silently refetched.
const (
	wagoPageCacheSchema = "lycheedev.wago-page.v1"
	wagoPageKeyPrefix   = "hotfix-wago-page/"
)

// wagoPageIdentity is the page identity a receipt is keyed by: everything that
// determines one page request plus the parser that read it. Record filters do
// not belong here; they are applied after parsing and share cached pages.
type wagoPageIdentity struct {
	Provider  string `json:"provider"`
	Parser    string `json:"parser"`
	Product   string `json:"product"`
	FullBuild string `json:"fullBuild"`
	Region    string `json:"region"`
	Locale    string `json:"locale"`
	Search    string `json:"search"`
	Page      int    `json:"page"`
}

func (i wagoPageIdentity) key() string {
	raw, err := json.Marshal(i)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

type wagoPageReceipt struct {
	Schema      string           `json:"schema"`
	Identity    wagoPageIdentity `json:"identity"`
	RequestURL  string           `json:"requestUrl"`
	Response    vault.BlobRef    `json:"response"`
	CapturedAt  time.Time        `json:"capturedAt"`
	CurrentPage int              `json:"currentPage"`
	LastPage    int              `json:"lastPage"`
	PerPage     int              `json:"perPage"`
	Total       int64            `json:"total"`
	NextPageURL string           `json:"nextPageUrl,omitempty"`
}

// loadWagoPage reads one verified receipt. Missing is ErrHotfixCacheMiss;
// every inconsistency is ErrHotfixCacheCorrupt so callers never refetch over a
// broken cache by accident.
func loadWagoPage(ctx context.Context, root string, identity wagoPageIdentity) (wagoPageReceipt, []byte, error) {
	type loaded struct {
		receipt wagoPageReceipt
		raw     []byte
	}
	result, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (loaded, error) {
		document, err := metadata.ReadDocument(ctx, wagoPageKeyPrefix+identity.key())
		if errors.Is(err, vault.ErrMissingRecord) {
			return loaded{}, ErrHotfixCacheMiss
		}
		if err != nil {
			return loaded{}, err
		}
		var receipt wagoPageReceipt
		if json.Unmarshal(document.Value, &receipt) != nil || receipt.Schema != wagoPageCacheSchema || receipt.Identity.key() != identity.key() {
			return loaded{}, fmt.Errorf("%w: receipt identity mismatch for page %d", ErrHotfixCacheCorrupt, identity.Page)
		}
		raw, err := store.ReadBlob(ctx, receipt.Response, wagoMaxPageBytes)
		if err != nil {
			return loaded{}, fmt.Errorf("%w: %v", ErrHotfixCacheCorrupt, err)
		}
		sum := sha256.Sum256(raw)
		if hex.EncodeToString(sum[:]) != receipt.Response.SHA256 || int64(len(raw)) != receipt.Response.Bytes {
			return loaded{}, fmt.Errorf("%w: response bytes do not match receipt for page %d", ErrHotfixCacheCorrupt, identity.Page)
		}
		return loaded{receipt: receipt, raw: raw}, nil
	})
	if err != nil {
		// A workspace without any receipts is simply a cache miss; everything
		// else that fails verification is an explicit cache corruption error.
		if errors.Is(err, vault.ErrMissingRecord) {
			return wagoPageReceipt{}, nil, ErrHotfixCacheMiss
		}
		return wagoPageReceipt{}, nil, err
	}
	return result.receipt, result.raw, nil
}

// storeWagoPage publishes one receipt. Concurrent writers of the same identity
// are accepted only when they stored identical bytes; differing pages for one
// identity mean the source shifted and are refused.
func storeWagoPage(ctx context.Context, root string, identity wagoPageIdentity, requestURL string, raw []byte, page WagoPage, capturedAt time.Time) (wagoPageReceipt, error) {
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (wagoPageReceipt, error) {
		blob, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: wagoMaxPageBytes})
		if err != nil {
			return wagoPageReceipt{}, fmt.Errorf("%w: %v", ErrHotfixCacheWrite, err)
		}
		receipt := wagoPageReceipt{
			Schema:      wagoPageCacheSchema,
			Identity:    identity,
			RequestURL:  requestURL,
			Response:    blob,
			CapturedAt:  capturedAt,
			CurrentPage: page.CurrentPage,
			LastPage:    page.LastPage,
			PerPage:     page.PerPage,
			Total:       page.Total,
			NextPageURL: page.NextPageURL,
		}
		value, err := json.Marshal(receipt)
		if err != nil {
			return wagoPageReceipt{}, err
		}
		key := wagoPageKeyPrefix + identity.key()
		same := func(document json.RawMessage) bool {
			var existing wagoPageReceipt
			return json.Unmarshal(document, &existing) == nil &&
				existing.Schema == wagoPageCacheSchema &&
				existing.Identity.key() == identity.key() &&
				existing.Response == blob
		}
		var generation int64
		document, err := metadata.ReadDocument(ctx, key)
		switch {
		case err == nil:
			generation = document.Generation
			if same(document.Value) {
				return receipt, nil
			}
		case errors.Is(err, vault.ErrMissingRecord):
		default:
			return wagoPageReceipt{}, err
		}
		err = metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: value, ExpectedGeneration: generation})
		if errors.Is(err, vault.ErrGeneration) {
			document, readErr := metadata.ReadDocument(ctx, key)
			if readErr == nil && same(document.Value) {
				return receipt, nil
			}
			return wagoPageReceipt{}, fmt.Errorf("%w: concurrent pages differ for one page identity", ErrHotfixPageDrift)
		}
		if err != nil {
			return wagoPageReceipt{}, fmt.Errorf("%w: %v", ErrHotfixCacheWrite, err)
		}
		return receipt, nil
	})
}
