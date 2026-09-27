package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/luals"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// v2 requires the LuaLS workspace-readiness barrier before synthetic API
// definition queries. v1 could persist an empty, prematurely complete result.
const semanticCacheSchema = "lycheedev.source-semantic.v2"
const maxSemanticCacheBytes = 4 << 20

type semanticCacheRecord struct {
	Schema   string             `json:"schema"`
	Key      string             `json:"key"`
	RowsHash string             `json:"rowsHash"`
	Rows     []ResearchRelation `json:"rows"`
}

func (b *Browser) cachedSemantic(ctx context.Context, pin selection.SourcePin, recordsHash string, env environment.Manifest, runtime *luals.Runtime, symbol SymbolMatch, direction string, compute func() ([]ResearchRelation, string)) ([]ResearchRelation, string) {
	if runtime == nil || symbol.ID == "" {
		return compute()
	}
	parts, err := json.Marshal([]string{semanticCacheSchema, pin.Repository, pin.Product, pin.ExactCommit, recordsHash, env.Identity.Repository, env.Identity.Commit, env.InputSHA256, env.DefinitionsSHA256, runtime.Identity.Version, runtime.Identity.SHA256, symbol.ID, direction})
	if err != nil {
		return compute()
	}
	h := sha256.Sum256(parts)
	key := hex.EncodeToString(h[:])
	dir := filepath.Join(b.store.Root(), "source", "v1", "semantics")
	filename := filepath.Join(dir, key+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:semantic:"+key)
	if err != nil {
		return compute()
	}
	defer lease.Close()
	if cached, err := readSemanticCache(filename, key); err == nil {
		return cached, ""
	}
	rows, reason := compute()
	if reason != "" || ctx.Err() != nil {
		return rows, reason
	}
	_ = b.writeSemanticCache(ctx, dir, filename, key, rows)
	return rows, reason
}

func readSemanticCache(filename, key string) ([]ResearchRelation, error) {
	f, err := os.Open(filename)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, maxSemanticCacheBytes+1))
	if err != nil {
		return nil, err
	}
	if len(raw) > maxSemanticCacheBytes {
		return nil, ErrSourceBudget
	}
	var record semanticCacheRecord
	if err := json.Unmarshal(raw, &record); err != nil {
		return nil, err
	}
	if record.Schema != semanticCacheSchema || record.Key != key {
		return nil, ErrSourceIntegrity
	}
	rowsRaw, err := json.Marshal(record.Rows)
	if err != nil {
		return nil, err
	}
	h := sha256.Sum256(rowsRaw)
	if hex.EncodeToString(h[:]) != record.RowsHash {
		return nil, ErrSourceIntegrity
	}
	return record.Rows, nil
}

func (b *Browser) writeSemanticCache(ctx context.Context, dir, filename, key string, rows []ResearchRelation) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	rowsRaw, err := json.Marshal(rows)
	if err != nil {
		return err
	}
	h := sha256.Sum256(rowsRaw)
	raw, err := json.Marshal(semanticCacheRecord{Schema: semanticCacheSchema, Key: key, RowsHash: hex.EncodeToString(h[:]), Rows: rows})
	if err != nil {
		return err
	}
	if len(raw) > maxSemanticCacheBytes {
		return ErrSourceBudget
	}
	capacity, err := b.reserveSourceCapacity(ctx, int64(len(raw)), filename)
	if err != nil {
		return err
	}
	defer capacity.Close()
	stage, err := os.CreateTemp(dir, ".semantic-")
	if err != nil {
		return err
	}
	defer os.Remove(stage.Name())
	if _, err := stage.Write(raw); err != nil {
		stage.Close()
		return err
	}
	if err := stage.Sync(); err != nil {
		stage.Close()
		return err
	}
	if err := stage.Close(); err != nil {
		return err
	}
	if old, err := os.Lstat(filename); err == nil {
		if !old.Mode().IsRegular() {
			return ErrSourceIntegrity
		}
		if err := os.Remove(filename); err != nil {
			return err
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	return os.Rename(stage.Name(), filename)
}
