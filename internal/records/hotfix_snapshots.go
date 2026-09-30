package records

import (
	"context"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type hotfixSnapshot struct {
	source evidence.CaptureRef
	raw    []byte
	layout uint32
	tables map[uint32][]CacheEntry
}

type hotfixSnapshots struct {
	store    *vault.Store
	metadata *vault.Metadata
	pin      selection.DataPin
	budget   *resource.Budget
	captures map[string]*hotfixSnapshot
	bytes    int64
}

func newHotfixSnapshots(store *vault.Store, metadata *vault.Metadata, pin selection.DataPin, budget *resource.Budget) *hotfixSnapshots {
	return &hotfixSnapshots{store: store, metadata: metadata, pin: pin, budget: budget, captures: map[string]*hotfixSnapshot{}}
}

func (h *hotfixSnapshots) load(ctx context.Context, id string) (*hotfixSnapshot, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if cached := h.captures[id]; cached != nil {
		return cached, nil
	}
	if len(h.captures) >= 16 {
		return nil, ErrCacheLimit
	}
	parts := strings.Split(h.pin.FullBuild, ".")
	build, err := strconv.ParseUint(parts[len(parts)-1], 10, 32)
	if err != nil {
		return nil, ErrCacheBuild
	}
	archive := evidence.OpenArchive(h.store, h.metadata)
	source, err := archive.InspectCapture(ctx, id)
	if err != nil {
		return nil, err
	}
	if source.Provenance.Kind != "hotfix-cache" || !source.Complete || source.Truncated {
		return nil, ErrCacheIdentity
	}
	origin, err := selection.OpenPinner(h.metadata).ReadPinnedSet(ctx, source.Provenance.Snapshot)
	if err != nil {
		return nil, err
	}
	if origin.Data == nil || *origin.Data != h.pin || source.Provenance.DataBuild != h.pin.FullBuild {
		return nil, ErrCacheIdentity
	}
	if source.Blob.Bytes < 0 || source.Blob.Bytes > (128<<20)-h.bytes {
		return nil, ErrCacheLimit
	}
	if err = h.budget.Charge(resource.Cost{RetainedBytes: source.Blob.Bytes, MetadataBytes: source.Blob.Bytes}); err != nil {
		return nil, err
	}
	raw, err := h.store.ReadBlob(ctx, source.Blob, 128<<20)
	if err != nil {
		return nil, err
	}
	_, layout, start, err := cacheLayout(ctx, raw, uint32(build))
	if err != nil {
		return nil, err
	}
	snapshot := &hotfixSnapshot{source: source, raw: raw, layout: layout, tables: map[uint32][]CacheEntry{}}
	err = walkCache(ctx, raw, start, layout, func(entry CacheEntry) error {
		if err := h.budget.Charge(resource.Cost{RetainedBytes: 384, MetadataBytes: 384, DecodeWork: 1}); err != nil {
			return err
		}
		snapshot.tables[entry.TableHash] = append(snapshot.tables[entry.TableHash], entry)
		return nil
	})
	if err != nil {
		return nil, err
	}
	h.bytes += int64(len(raw))
	h.captures[id] = snapshot
	return snapshot, nil
}
