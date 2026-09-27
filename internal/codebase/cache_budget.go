package codebase

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// The source budget covers materialized source/v1 bytes. Mirrors, durable
// evidence, public metadata and data caches are outside this namespace.
const sourceDerivedBudget int64 = 4 << 30
const maxSourceNodes = 500000

var sourceCacheKey = regexp.MustCompile(`^[0-9a-f]{64}$`)

type sourceCapacityLease struct {
	lease         *vault.Lease
	browser       *Browser
	baselineUsed  int64
	reserved      int64
	protectedPath string
	budget        int64
}

func (l *sourceCapacityLease) Close() error { return l.lease.Close() }

// Grow reserves more bytes while the same lock is held. Already written bytes
// are visible in source/v1; still-unwritten grants remain accounted, so a
// streaming writer cannot overcommit by growing before its last chunk is full.
func (l *sourceCapacityLease) Grow(ctx context.Context, additionalBytes int64) error {
	if additionalBytes < 0 {
		return fmt.Errorf("%w: negative source reservation", ErrSourceBudget)
	}
	used, err := sourcePathBytes(ctx, filepath.Join(l.browser.store.Root(), "source", "v1"))
	if err != nil {
		return err
	}
	materialized := used - l.baselineUsed
	if materialized < 0 {
		materialized = 0
	}
	outstanding := l.reserved - materialized
	if outstanding < 0 {
		outstanding = 0
	}
	if err := l.browser.ensureSourceCapacityLockedBudget(ctx, outstanding+additionalBytes, l.protectedPath, l.budget); err != nil {
		return err
	}
	l.reserved += additionalBytes
	return nil
}

// reserveSourceCapacity holds the source capacity lock until the caller has
// published or abandoned its bounded staged bytes. `additionalBytes` must be
// a hard upper bound on all bytes created before Close, including temporary
// files and any prior version left in quarantine during replacement.
func (b *Browser) reserveSourceCapacity(ctx context.Context, additionalBytes int64, protectedPath string) (*sourceCapacityLease, error) {
	return b.reserveSourceCapacityBudget(ctx, additionalBytes, protectedPath, sourceDerivedBudget)
}

func (b *Browser) reserveSourceCapacityBudget(ctx context.Context, additionalBytes int64, protectedPath string, budget int64) (*sourceCapacityLease, error) {
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree-capacity")
	if err != nil {
		return nil, err
	}
	if err := b.ensureSourceCapacityLockedBudget(ctx, additionalBytes, protectedPath, budget); err != nil {
		lease.Close()
		return nil, err
	}
	used, err := sourcePathBytes(ctx, filepath.Join(b.store.Root(), "source", "v1"))
	if err != nil {
		lease.Close()
		return nil, err
	}
	return &sourceCapacityLease{lease: lease, browser: b, baselineUsed: used, reserved: additionalBytes, protectedPath: protectedPath, budget: budget}, nil
}

// The caller already holds source:v1:worktree-capacity (e.g. checkout), so it
// must not try to acquire that lease again.
func (b *Browser) ensureSourceCapacityLocked(ctx context.Context, additionalBytes int64, protectedPath string) error {
	return b.ensureSourceCapacityLockedBudget(ctx, additionalBytes, protectedPath, sourceDerivedBudget)
}

func (b *Browser) ensureSourceCapacityLockedBudget(ctx context.Context, additionalBytes int64, protectedPath string, budget int64) error {
	if budget < 0 || budget > sourceDerivedBudget || additionalBytes < 0 || additionalBytes > budget {
		return fmt.Errorf("%w: invalid source reservation", ErrSourceBudget)
	}
	root := filepath.Join(b.store.Root(), "source", "v1")
	if protectedPath != "" {
		rel, err := filepath.Rel(root, protectedPath)
		if err != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			return fmt.Errorf("%w: invalid protected source path", ErrSourceIntegrity)
		}
	}
	used, err := sourcePathBytes(ctx, root)
	if err != nil {
		return err
	}
	if used+additionalBytes <= budget {
		return nil
	}
	candidates, err := b.reclaimableSourceCaches(ctx, protectedPath)
	if err != nil {
		return err
	}
	for _, candidate := range candidates {
		if err := ctx.Err(); err != nil {
			return err
		}
		if used+additionalBytes <= budget {
			return nil
		}
		lease, err := vault.TryAcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), candidate.lock)
		if errors.Is(err, vault.ErrLeaseBusy) {
			continue
		}
		if err != nil {
			return err
		}
		freed, removeErr := reclaimSourceCache(ctx, candidate)
		lease.Close()
		if removeErr != nil {
			return removeErr
		}
		used -= freed
	}
	if used+additionalBytes > budget {
		return fmt.Errorf("%w: source/v1 requires %d bytes, budget %d", ErrSourceBudget, used+additionalBytes, budget)
	}
	return nil
}

func sourcePathBytes(ctx context.Context, root string) (int64, error) {
	var total int64
	nodes := 0
	err := filepath.WalkDir(root, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		nodes++
		if nodes > maxSourceNodes {
			return fmt.Errorf("%w: source/v1 node budget", ErrSourceBudget)
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("%w: source/v1 link %s", ErrSourceIntegrity, path)
		}
		if entry.IsDir() {
			return nil
		}
		if !entry.Type().IsRegular() {
			return fmt.Errorf("%w: source/v1 special file %s", ErrSourceIntegrity, path)
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		total += info.Size()
		return nil
	})
	if errors.Is(err, os.ErrNotExist) {
		return 0, nil
	}
	return total, err
}

type sourceCacheCandidate struct {
	path     string
	lock     string
	kind     string
	modified time.Time
}

func (b *Browser) reclaimableSourceCaches(ctx context.Context, protected string) ([]sourceCacheCandidate, error) {
	root := filepath.Join(b.store.Root(), "source", "v1")
	result := []sourceCacheCandidate{}
	for _, kind := range []string{"environments", "semantics"} {
		parent := filepath.Join(root, kind)
		entries, err := os.ReadDir(parent)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, err
		}
		for _, entry := range entries {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			name := entry.Name()
			key := name
			if kind == "semantics" {
				key = strings.TrimSuffix(name, ".json")
			}
			if !sourceCacheKey.MatchString(key) || kind == "environments" && !entry.IsDir() || kind == "semantics" && (!strings.HasSuffix(name, ".json") || !entry.Type().IsRegular()) {
				// Unknown cache content is counted but never deleted automatically.
				continue
			}
			path := filepath.Join(parent, name)
			if protected != "" && strings.EqualFold(filepath.Clean(path), filepath.Clean(protected)) {
				continue
			}
			info, err := entry.Info()
			if err != nil {
				return nil, err
			}
			lock := "source:v1:environment:" + key
			if kind == "semantics" {
				lock = "source:v1:semantic:" + key
			}
			result = append(result, sourceCacheCandidate{path: path, lock: lock, kind: kind, modified: info.ModTime()})
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].modified.Before(result[j].modified) })
	return result, nil
}

func reclaimSourceCache(ctx context.Context, candidate sourceCacheCandidate) (int64, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if candidate.kind == "environments" {
		entries, err := os.ReadDir(candidate.path)
		if errors.Is(err, os.ErrNotExist) {
			return 0, nil
		}
		if err != nil {
			return 0, err
		}
		allowed := map[string]bool{"manifest.json": true, "api.d.lua": true, "facts.jsonl": true, "mappings.json": true}
		for _, entry := range entries {
			if !allowed[entry.Name()] || !entry.Type().IsRegular() {
				return 0, fmt.Errorf("%w: unrecognized environment cache content", ErrSourceIntegrity)
			}
		}
		freed, err := sourcePathBytes(ctx, candidate.path)
		if err != nil {
			return 0, err
		}
		if err := os.RemoveAll(candidate.path); err != nil {
			return 0, err
		}
		return freed, nil
	}
	info, err := os.Lstat(candidate.path)
	if errors.Is(err, os.ErrNotExist) {
		return 0, nil
	}
	if err != nil {
		return 0, err
	}
	if !info.Mode().IsRegular() {
		return 0, fmt.Errorf("%w: unrecognized semantic cache content", ErrSourceIntegrity)
	}
	if err := os.Remove(candidate.path); err != nil {
		return 0, err
	}
	return info.Size(), nil
}
