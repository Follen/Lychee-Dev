package codebase

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const defaultSourceWorktreeBudget int64 = 4 << 30

type SourcePruneResult struct {
	BudgetBytes int64    `json:"budgetBytes"`
	BeforeBytes int64    `json:"beforeBytes"`
	AfterBytes  int64    `json:"afterBytes"`
	Removed     []string `json:"removed"`
	Skipped     []string `json:"skipped"`
	Complete    bool     `json:"complete"`
}

type reclaimCandidate struct {
	pin            selection.SourcePin
	path, manifest string
	modified       time.Time
}

// PruneSourceWorktrees is an explicit source-only cleanup operation. Pins,
// captures and JSON facts are outside its ownership and are never removed.
func PruneSourceWorktrees(ctx context.Context, root string, budgetBytes int64) (SourcePruneResult, error) {
	s, err := vault.OpenStore(root)
	if err != nil {
		return SourcePruneResult{}, err
	}
	return OpenBrowser(s).pruneWorktrees(ctx, budgetBytes, "")
}

func (b *Browser) pruneWorktrees(ctx context.Context, budgetBytes int64, excludeCommit string) (SourcePruneResult, error) {
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree-capacity")
	if err != nil {
		return SourcePruneResult{}, err
	}
	defer lease.Close()
	if err := b.recoverOrphanWorktrees(ctx, selection.SourcePin{}); err != nil {
		return SourcePruneResult{}, err
	}
	return b.pruneWorktreesLocked(ctx, budgetBytes, excludeCommit)
}

// Caller holds the global source-worktree capacity lease. Manifest accounting,
// eviction and new checkout reservation must be one serialized transaction.
func (b *Browser) pruneWorktreesLocked(ctx context.Context, budgetBytes int64, excludeCommit string) (SourcePruneResult, error) {
	result := SourcePruneResult{BudgetBytes: budgetBytes, Removed: []string{}, Skipped: []string{}}
	if budgetBytes < 0 {
		return result, fmt.Errorf("%w: codebase.source_budget_invalid", ErrSourceBudget)
	}
	worktrees := filepath.Join(b.store.Root(), "source", "v1", "worktrees")
	// Count actual checkout bytes, including unverified orphans that must be
	// preserved. A manifest alone cannot describe a failed checkout or a late
	// local edit. sourcePathBytes refuses links and special files without
	// following them.
	actualBytes, err := sourcePathBytes(ctx, worktrees)
	if err != nil {
		return result, err
	}
	result.BeforeBytes, result.AfterBytes = actualBytes, actualBytes
	root := filepath.Join(b.store.Root(), "source", "v1", "manifests")
	candidates := []reclaimCandidate{}
	err = filepath.WalkDir(root, func(name string, entry os.DirEntry, walkErr error) error {
		if errors.Is(walkErr, os.ErrNotExist) {
			return nil
		}
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("%w: codebase.source_manifest_link", ErrSourceIntegrity)
		}
		if !strings.HasSuffix(name, ".json") {
			return nil
		}
		f, err := os.Open(name)
		if err != nil {
			return err
		}
		raw, readErr := io.ReadAll(io.LimitReader(f, 64<<10))
		closeErr := f.Close()
		if readErr != nil || closeErr != nil {
			return errors.Join(readErr, closeErr)
		}
		var m worktreeManifest
		if err := json.Unmarshal(raw, &m); err != nil {
			return fmt.Errorf("%w: codebase.source_manifest_json: %v", ErrSourceIntegrity, err)
		}
		if m.Schema != worktreeSchema || m.State != "ready" || !objectID(m.Commit) || m.Repository != filepath.Base(filepath.Dir(name)) || m.Commit+".json" != filepath.Base(name) || m.Bytes < 0 {
			return fmt.Errorf("%w: codebase.source_manifest_invalid", ErrSourceIntegrity)
		}
		if _, err := LookupRepository(m.Repository); err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		pin := selection.SourcePin{Repository: m.Repository, ExactCommit: m.Commit, ParserRevision: ParserRevision}
		path := b.worktreePath(pin)
		candidates = append(candidates, reclaimCandidate{pin: pin, path: path, manifest: name, modified: info.ModTime()})
		return nil
	})
	if errors.Is(err, os.ErrNotExist) {
		result.Complete = result.AfterBytes <= budgetBytes
		return result, nil
	}
	if err != nil {
		return result, err
	}
	sort.Slice(candidates, func(i, j int) bool { return candidates[i].modified.Before(candidates[j].modified) })
	for _, candidate := range candidates {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		// A stale manifest still needs its Git registration and accounting
		// removed when actual checkout usage is already below the limit.
		_, pathErr := os.Lstat(candidate.path)
		if pathErr == nil && result.AfterBytes <= budgetBytes {
			continue
		}
		if pathErr != nil && !errors.Is(pathErr, os.ErrNotExist) {
			return result, pathErr
		}
		if candidate.pin.ExactCommit == excludeCommit {
			result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":in-use")
			continue
		}
		tryCtx, cancel := context.WithTimeout(ctx, 50*time.Millisecond)
		lease, err := vault.AcquireLease(tryCtx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree:"+candidate.pin.Repository+":"+candidate.pin.ExactCommit)
		cancel()
		if err != nil {
			if ctx.Err() != nil {
				return result, ctx.Err()
			}
			result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":in-use")
			continue
		}
		var pruneErr error
		func() {
			defer lease.Close()
			if _, err := os.Lstat(candidate.path); errors.Is(err, os.ErrNotExist) {
				repoLease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:repo:"+candidate.pin.Repository)
				if err != nil {
					pruneErr = err
					return
				}
				_, pruneErr = gitBytes(ctx, b.mirror(candidate.pin.Repository), 4096, "worktree", "prune", "--expire=now")
				repoLease.Close()
				if pruneErr != nil {
					return
				}
				if err := os.Remove(candidate.manifest); err == nil {
					result.Removed = append(result.Removed, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":stale-manifest")
					return
				}
			}
			if _, err := b.verifyWorktree(ctx, candidate.pin, candidate.path); err != nil {
				if ctx.Err() != nil {
					pruneErr = ctx.Err()
					return
				}
				result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":modified")
				return
			}
			repoLease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:repo:"+candidate.pin.Repository)
			if err != nil {
				if ctx.Err() != nil {
					pruneErr = ctx.Err()
					return
				}
				result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":lock")
				return
			}
			defer repoLease.Close()
			if _, err := gitBytes(ctx, b.mirror(candidate.pin.Repository), 4096, "worktree", "remove", candidate.path); err != nil {
				if ctx.Err() != nil {
					pruneErr = ctx.Err()
					return
				}
				result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":git-error")
				return
			}
			if err := os.Remove(candidate.manifest); err != nil {
				result.Skipped = append(result.Skipped, candidate.pin.Repository+":"+candidate.pin.ExactCommit+":manifest-error")
				return
			}
			result.Removed = append(result.Removed, candidate.pin.Repository+":"+candidate.pin.ExactCommit)
			result.AfterBytes, pruneErr = sourcePathBytes(ctx, worktrees)
		}()
		if pruneErr != nil {
			return result, pruneErr
		}
	}
	result.Complete = result.AfterBytes <= budgetBytes
	return result, nil
}

func (b *Browser) recoverOrphanWorktrees(ctx context.Context, held selection.SourcePin) error {
	root := filepath.Join(b.store.Root(), "source", "v1", "worktrees")
	rootEntries, err := os.ReadDir(root)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	known := map[string]bool{}
	for _, repo := range Repositories() {
		known[repo.Key] = true
	}
	for _, entry := range rootEntries {
		if !entry.IsDir() || !known[entry.Name()] {
			return fmt.Errorf("%w: codebase.source_orphan_untrusted", ErrSourceIntegrity)
		}
	}
	for _, repo := range Repositories() {
		dir := filepath.Join(root, repo.Key)
		entries, err := os.ReadDir(dir)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return err
		}
		for _, entry := range entries {
			if err := ctx.Err(); err != nil {
				return err
			}
			if !entry.IsDir() || !objectID(entry.Name()) {
				return fmt.Errorf("%w: codebase.source_orphan_untrusted", ErrSourceIntegrity)
			}
			pin := selection.SourcePin{Repository: repo.Key, ExactCommit: entry.Name(), ParserRevision: ParserRevision}
			if pin.Repository == held.Repository && pin.ExactCommit == held.ExactCommit {
				continue
			}
			if _, err := os.Lstat(b.worktreeManifestPath(pin)); err == nil {
				continue
			} else if !errors.Is(err, os.ErrNotExist) {
				return err
			}
			lease, err := vault.TryAcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree:"+pin.Repository+":"+pin.ExactCommit)
			if errors.Is(err, vault.ErrLeaseBusy) {
				// Another request owns this checkout. Its bytes remain charged to
				// the source budget; it is not a reclaim candidate.
				continue
			}
			if err != nil {
				return err
			}
			manifest, err := b.verifyWorktree(ctx, pin, b.worktreePath(pin))
			if err == nil {
				err = b.publishWorktreeManifest(pin, manifest)
			}
			lease.Close()
			if err != nil {
				if ctx.Err() != nil {
					return ctx.Err()
				}
				if errors.Is(err, ErrSourceIntegrity) {
					// Keep a dirty/crash-interrupted non-target checkout intact.
					// AcquireWorktree verifies the selected target below, and the
					// actual-byte budget still accounts for this orphan.
					continue
				}
				return fmt.Errorf("%w: codebase.source_orphan_unverified: %v", ErrSourceIntegrity, err)
			}
		}
	}
	return nil
}

func (b *Browser) estimateWorktreeBytes(ctx context.Context, pin selection.SourcePin) (int64, error) {
	tree, err := gitBytes(ctx, b.mirror(pin.Repository), 64<<20, "ls-tree", "-r", "-l", "-z", "--full-tree", pin.ExactCommit)
	if err != nil {
		return 0, err
	}
	var total int64
	for _, entry := range bytes.Split(tree, []byte{0}) {
		if len(entry) == 0 {
			continue
		}
		header, _, ok := bytes.Cut(entry, []byte{'\t'})
		fields := strings.Fields(string(header))
		if !ok || len(fields) != 4 {
			return 0, fmt.Errorf("%w: codebase.invalid_tree_record", ErrSourceIntegrity)
		}
		if fields[0] != "100644" && fields[0] != "100755" {
			return 0, fmt.Errorf("%w: codebase.worktree_unsupported_entry: %s", ErrSourceIntegrity, fields[0])
		}
		size, err := strconv.ParseInt(fields[3], 10, 64)
		if err != nil || size < 0 || size > maxSourceBytes*4 {
			return 0, fmt.Errorf("%w: codebase.worktree_file_budget", ErrSourceBudget)
		}
		total += size
		if total > defaultSourceWorktreeBudget {
			return 0, fmt.Errorf("%w: codebase.worktree_budget", ErrSourceBudget)
		}
	}
	return total, nil
}
