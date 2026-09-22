package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type HotfixRequest struct {
	File     string
	From     string
	Table    string
	Offline  bool
	Filter   CacheFilter
	MaxBytes int64
}

func (q HotfixRequest) Validate() error {
	if q.Table != "" && q.Filter.TableHash != nil {
		return fmt.Errorf("%w: select --table or --table-hash, not both", ErrCacheIdentity)
	}
	if (q.File == "") == (q.From == "") || q.Filter.Build != 0 || q.Filter.AfterIndex != nil && q.From == "" {
		return fmt.Errorf("%w: choose --file or --from; pagination requires an archived --from", ErrCacheIdentity)
	}
	if q.Filter.Limit < 1 || q.Filter.Limit > 200 || q.MaxBytes < 1 || q.MaxBytes > 512<<20 {
		return ErrCacheLimit
	}
	return nil
}

type HotfixResult struct {
	Snapshot   string              `json:"snapshot"`
	Source     evidence.CaptureRef `json:"source"`
	Page       CachePage           `json:"page"`
	Sources    *DefinitionBundle   `json:"sources,omitempty"`
	Definition *schema.Definition  `json:"definition,omitempty"`
}

type HotfixReading struct {
	Result  HotfixResult
	Capture evidence.CaptureRef
}

// InspectHotfix keeps local observations independent of static DB2. The cache
// attests only a build number, not product, locale, server coverage or region.
// The latter remain the caller's selected context. No old workspace is searched.
func InspectHotfix(ctx context.Context, root, snapshot string, q HotfixRequest) (HotfixReading, error) {
	if err := q.Validate(); err != nil {
		return HotfixReading{}, err
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (HotfixReading, error) {
		pinner, archive := selection.OpenPinner(m), evidence.OpenArchive(s, m)
		pin, err := pinner.ReadPinnedSet(ctx, snapshot)
		if err != nil {
			return HotfixReading{}, err
		}
		if pin.Data == nil {
			return HotfixReading{}, ErrCacheIdentity
		}
		if _, _, err := selection.DataIdentity(*pin.Data); err != nil {
			return HotfixReading{}, err
		}
		parts := strings.Split(pin.Data.FullBuild, ".")
		build, err := strconv.ParseUint(parts[3], 10, 32)
		if err != nil {
			return HotfixReading{}, ErrCacheBuild
		}
		q.Filter.Build = uint32(build)
		var raw []byte
		var source evidence.CaptureRef
		if q.From != "" {
			source, raw, err = archive.FetchCapture(ctx, q.From, q.MaxBytes)
			if err != nil {
				return HotfixReading{}, err
			}
			if source.Provenance.Kind != "hotfix-cache" || !source.Complete || source.Truncated || source.MediaType != "application/octet-stream" {
				return HotfixReading{}, ErrCacheIdentity
			}
			origin, err := pinner.ReadPinnedSet(ctx, source.Provenance.Snapshot)
			if err != nil {
				return HotfixReading{}, err
			}
			if origin.Data == nil || *origin.Data != *pin.Data || source.Provenance.DataBuild != pin.Data.FullBuild {
				return HotfixReading{}, ErrCacheIdentity
			}
		} else {
			raw, err = readCacheFile(ctx, q.File, q.MaxBytes)
			if err != nil {
				return HotfixReading{}, err
			}
		}
		var bundle *DefinitionBundle
		var definition *schema.Definition
		if q.Table != "" {
			prepared, err := OpenDefinitions(s, m).Prepare(ctx, pin.Data.DefinitionCommit, q.Table, q.Offline)
			if err != nil {
				return HotfixReading{}, err
			}
			manifestRaw, err := s.ReadBlob(ctx, prepared.Manifest, 4<<20)
			if err != nil {
				return HotfixReading{}, err
			}
			manifest, err := schema.ParseManifest(ctx, manifestRaw)
			if err != nil {
				return HotfixReading{}, err
			}
			identity, err := manifest.SelectHash(ctx, prepared.Identity.Hash)
			if err != nil {
				return HotfixReading{}, err
			}
			if identity != prepared.Identity {
				return HotfixReading{}, ErrDefinitionIdentity
			}
			definitionRaw, err := s.ReadBlob(ctx, prepared.Definition, 4<<20)
			if err != nil {
				return HotfixReading{}, err
			}
			doc, err := schema.Parse(ctx, definitionRaw)
			if err != nil {
				return HotfixReading{}, err
			}
			selected, err := doc.Select(ctx, pin.Data.FullBuild, "")
			if err != nil {
				return HotfixReading{}, err
			}
			bundle, definition = &prepared, &selected
			q.Filter.TableHash = &identity.Hash
		}
		page, err := InspectCache(ctx, raw, q.Filter)
		if err != nil {
			return HotfixReading{}, err
		}
		if definition != nil {
			fieldsBytes := 0
			for i := range page.Entries {
				entry := &page.Entries[i]
				entry.DecodeState = "no_payload"
				if entry.PayloadBytes == 0 {
					continue
				}
				if page.RecordLayout >= 7 && entry.Status != 1 || page.RecordLayout < 7 && entry.Status == 0 {
					entry.DecodeState = "not_valid"
					continue
				}
				entry.Fields, err = DecodeHotfixFields(ctx, raw[int(entry.PayloadOffset):int(entry.PayloadOffset)+entry.PayloadBytes], entry.RecordID, *definition)
				if err != nil {
					return HotfixReading{}, fmt.Errorf("record index %d: %w", entry.Index, err)
				}
				serialized, err := json.Marshal(entry.Fields)
				if err != nil {
					return HotfixReading{}, err
				}
				if len(serialized) > (8<<20)-fieldsBytes {
					return HotfixReading{}, ErrCacheLimit
				}
				fieldsBytes += len(serialized)
				entry.DecodeState = "decoded"
			}
		}
		sum := sha256.Sum256(raw)
		digest := hex.EncodeToString(sum[:])
		scope := "local-cache:" + pin.Data.Language
		if c := pin.Changes; c != nil && (c.Provider != "dbcache" || c.SHA256 != digest || c.Product != pin.Data.Product || c.Region != pin.Data.Region || c.FullBuild != pin.Data.FullBuild || c.Scope != scope) {
			return HotfixReading{}, ErrCacheIdentity
		}
		if q.From == "" {
			source, err = archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(raw), MaxBytes: q.MaxBytes, MediaType: "application/octet-stream", Complete: true, Provenance: evidence.Provenance{Kind: "hotfix-cache", Locator: "dbcache:sha256:" + digest, Snapshot: snapshot, DataBuild: pin.Data.FullBuild}})
			if err != nil {
				return HotfixReading{}, err
			}
		}
		if pin.Changes == nil {
			pin, err = pinner.PinSelection(ctx, selection.SelectionSpec{Parent: snapshot, Changes: &selection.ChangePin{Provider: "dbcache", Product: pin.Data.Product, Region: pin.Data.Region, FullBuild: pin.Data.FullBuild, Scope: scope, SHA256: digest, FetchedAt: source.CapturedAt}})
			if err != nil {
				return HotfixReading{}, err
			}
		}
		result := HotfixResult{Snapshot: pin.ID, Source: source, Page: page, Sources: bundle, Definition: definition}
		body, err := json.Marshal(result)
		if err != nil {
			return HotfixReading{}, err
		}
		capture, err := archive.CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(body), MaxBytes: 32 << 20, MediaType: "application/json", Complete: page.Complete, Truncated: page.Truncated, Provenance: evidence.Provenance{Kind: "hotfix-records", Locator: "dbcache:sha256:" + digest, Snapshot: pin.ID, DataBuild: pin.Data.FullBuild}})
		if err != nil {
			return HotfixReading{}, err
		}
		return HotfixReading{Result: result, Capture: capture}, nil
	})
}

func readCacheFile(ctx context.Context, path string, maxBytes int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	before, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() {
		return nil, ErrCacheFormat
	}
	if before.Size() > maxBytes {
		return nil, ErrCacheLimit
	}
	var buffer bytes.Buffer
	chunk := make([]byte, 64<<10)
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		n, err := f.Read(chunk)
		if int64(buffer.Len())+int64(n) > maxBytes {
			return nil, ErrCacheLimit
		}
		buffer.Write(chunk[:n])
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
	}
	after, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if before.Size() != after.Size() || after.Size() != int64(buffer.Len()) || !before.ModTime().Equal(after.ModTime()) {
		return nil, fmt.Errorf("%w: cache changed while reading; retry from a stable copy", ErrCacheIdentity)
	}
	return buffer.Bytes(), nil
}
