package codebase

import (
	"bytes"
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const worktreeSchema = "lycheedev.source-worktree.v1"

type worktreeManifest struct {
	Schema     string `json:"schema"`
	Repository string `json:"repository"`
	Commit     string `json:"commit"`
	Tree       string `json:"tree"`
	Files      int    `json:"files"`
	Bytes      int64  `json:"bytes"`
	State      string `json:"state"`
}

// WorktreeLease protects a verified, detached checkout for a semantic request.
// Consumers must close it before a worktree can be reclaimed.
type WorktreeLease struct {
	path  string
	lease *vault.Lease
}

func (w *WorktreeLease) Path() string { return w.path }
func (w *WorktreeLease) Close() error { return w.lease.Close() }

func (b *Browser) worktreePath(pin selection.SourcePin) string {
	return filepath.Join(b.store.Root(), "source", "v1", "worktrees", pin.Repository, pin.ExactCommit)
}

func (b *Browser) worktreeManifestPath(pin selection.SourcePin) string {
	return filepath.Join(b.store.Root(), "source", "v1", "manifests", pin.Repository, pin.ExactCommit+".json")
}

// AcquireWorktree creates and verifies an exact detached checkout. The Git
// object remains the source of truth; checkout bytes are checked against every
// regular blob before a language server can inspect them.
func (b *Browser) AcquireWorktree(ctx context.Context, pin selection.SourcePin) (*WorktreeLease, error) {
	if _, err := LookupRepository(pin.Repository); err != nil {
		return nil, err
	}
	if !objectID(pin.ExactCommit) || pin.ParserRevision != ParserRevision {
		return nil, errors.New("codebase.invalid_source_pin")
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree:"+pin.Repository+":"+pin.ExactCommit)
	if err != nil {
		return nil, err
	}
	ready := false
	defer func() {
		if !ready {
			lease.Close()
		}
	}()
	capacity, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:worktree-capacity")
	if err != nil {
		return nil, err
	}
	defer capacity.Close()
	path := b.worktreePath(pin)
	if err := b.recoverOrphanWorktrees(ctx, pin); err != nil {
		return nil, err
	}
	estimate := int64(-1)
	_, manifestErr := os.Lstat(b.worktreeManifestPath(pin))
	if errors.Is(manifestErr, os.ErrNotExist) {
		computed, err := b.estimateWorktreeBytes(ctx, pin)
		if err != nil {
			return nil, err
		}
		estimate = computed
		space, err := b.pruneWorktreesLocked(ctx, defaultSourceWorktreeBudget-estimate, pin.ExactCommit)
		if err != nil {
			return nil, err
		}
		if !space.Complete {
			return nil, fmt.Errorf("%w: codebase.source_worktree_budget", ErrSourceBudget)
		}
	} else if manifestErr != nil {
		return nil, manifestErr
	}
	_, pathErr := os.Lstat(path)
	if errors.Is(pathErr, os.ErrNotExist) {
		// The global source budget also counts preserved fact maps and derived
		// semantic caches. Reserve checkout bytes plus bounded Git metadata.
		if estimate < 0 {
			computed, err := b.estimateWorktreeBytes(ctx, pin)
			if err != nil {
				return nil, err
			}
			estimate = computed
		}
		if err := b.ensureSourceCapacityLocked(ctx, estimate+(64<<10), path); err != nil {
			return nil, err
		}
		repoLease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:v1:repo:"+pin.Repository)
		if err != nil {
			return nil, err
		}
		defer repoLease.Close()
		if err := b.verifyCheckoutGitConfig(ctx, pin.Repository); err != nil {
			return nil, err
		}
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return nil, err
		}
		// Never execute hooks, filters from inherited configuration, or submodule
		// actions. Explicit LF checkout prevents text=auto from converting
		// committed LF blobs to native CRLF on Windows. Repository attributes
		// can still request other conversions; exact blob verification below
		// rejects those rather than silently changing the source identity.
		if _, err := gitBytes(ctx, b.mirror(pin.Repository), 4096, "-c", "core.autocrlf=false", "-c", "core.eol=lf", "-c", "core.safecrlf=true", "-c", "submodule.recurse=false", "worktree", "add", "--detach", path, pin.ExactCommit); err != nil {
			return nil, err
		}
	} else if pathErr != nil {
		return nil, pathErr
	} else if err := b.ensureSourceCapacityLocked(ctx, 0, path); err != nil {
		return nil, err
	}
	manifest, err := b.verifyWorktree(ctx, pin, path)
	if err != nil {
		return nil, err
	}
	if err := b.publishWorktreeManifest(pin, manifest); err != nil {
		return nil, err
	}
	ready = true
	return &WorktreeLease{path: path, lease: lease}, nil
}

func (b *Browser) publishWorktreeManifest(pin selection.SourcePin, manifest worktreeManifest) error {
	path := b.worktreeManifestPath(pin)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	raw, err := json.Marshal(manifest)
	if err != nil {
		return err
	}
	if existing, err := os.ReadFile(path); err == nil {
		if bytes.Equal(existing, raw) {
			return nil
		}
		return fmt.Errorf("%w: codebase.source_manifest_conflict", ErrSourceIntegrity)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".manifest-*.json")
	if err != nil {
		return err
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.Write(raw); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}

// A managed bare mirror may predate this source engine. Checkout can invoke
// filters from its local Git config before content verification, so accept only
// inert repository settings. In particular, filter, include, fsmonitor and
// external command settings cannot reach worktree add.
func (b *Browser) verifyCheckoutGitConfig(ctx context.Context, repository string) error {
	raw, err := gitBytes(ctx, b.mirror(repository), 64<<10, "config", "--local", "--list", "--null")
	if err != nil {
		return err
	}
	for _, entry := range bytes.Split(raw, []byte{0}) {
		if len(entry) == 0 {
			continue
		}
		key, _, ok := bytes.Cut(entry, []byte{'\n'})
		if !ok {
			return fmt.Errorf("%w: codebase.worktree_untrusted_git_config", ErrSourceIntegrity)
		}
		name := strings.ToLower(string(key))
		switch name {
		case "core.repositoryformatversion", "core.filemode", "core.bare", "core.logallrefupdates", "core.ignorecase", "core.precomposeunicode", "core.symlinks", "extensions.objectformat":
			continue
		}
		if strings.HasPrefix(name, "remote.") && (strings.HasSuffix(name, ".url") || strings.HasSuffix(name, ".fetch")) {
			continue
		}
		return fmt.Errorf("%w: codebase.worktree_untrusted_git_config: %s", ErrSourceIntegrity, name)
	}
	return nil
}

func (b *Browser) verifyWorktree(ctx context.Context, pin selection.SourcePin, checkout string) (worktreeManifest, error) {
	manifest := worktreeManifest{Schema: worktreeSchema, Repository: pin.Repository, Commit: pin.ExactCommit, State: "ready"}
	if err := verifyWorktreeGitBinding(checkout, b.mirror(pin.Repository)); err != nil {
		return manifest, err
	}
	var err error
	head, err := gitBytes(ctx, "", 128, "-C", checkout, "rev-parse", "--verify", "HEAD")
	if err != nil || strings.TrimSpace(string(head)) != pin.ExactCommit {
		return manifest, fmt.Errorf("%w: codebase.worktree_identity_mismatch", ErrSourceIntegrity)
	}
	registry, err := gitBytes(ctx, b.mirror(pin.Repository), 1<<20, "worktree", "list", "--porcelain")
	if err != nil {
		return manifest, err
	}
	registered := false
	for _, block := range bytes.Split(registry, []byte("\n\n")) {
		lines := strings.Split(string(block), "\n")
		if len(lines) < 3 || !strings.HasPrefix(lines[0], "worktree ") {
			continue
		}
		if !strings.EqualFold(filepath.Clean(strings.TrimPrefix(lines[0], "worktree ")), filepath.Clean(checkout)) {
			continue
		}
		for _, line := range lines {
			if line == "HEAD "+pin.ExactCommit {
				registered = true
			}
		}
	}
	if !registered {
		return manifest, fmt.Errorf("%w: codebase.worktree_identity_mismatch", ErrSourceIntegrity)
	}
	resolved, err := gitBytes(ctx, b.mirror(pin.Repository), 128, "rev-parse", "--verify", pin.ExactCommit+"^{tree}")
	if err != nil {
		return manifest, err
	}
	manifest.Tree = strings.TrimSpace(string(resolved))
	tree, err := gitBytes(ctx, b.mirror(pin.Repository), 64<<20, "ls-tree", "-r", "-l", "-z", "--full-tree", pin.ExactCommit)
	if err != nil {
		return manifest, err
	}
	tracked := map[string]bool{}
	for _, entry := range bytes.Split(tree, []byte{0}) {
		if len(entry) == 0 {
			continue
		}
		header, name, ok := bytes.Cut(entry, []byte{'\t'})
		fields := strings.Fields(string(header))
		if !ok || len(fields) != 4 || !sourcePath(string(name)) {
			return manifest, fmt.Errorf("%w: codebase.invalid_tree_record", ErrSourceIntegrity)
		}
		if fields[0] != "100644" && fields[0] != "100755" {
			return manifest, fmt.Errorf("%w: codebase.worktree_unsupported_entry: %s", ErrSourceIntegrity, name)
		}
		if !objectID(fields[2]) {
			return manifest, fmt.Errorf("%w: codebase.invalid_tree_record", ErrSourceIntegrity)
		}
		size, err := strconv.ParseInt(fields[3], 10, 64)
		if err != nil || size < 0 || size > maxSourceBytes*4 {
			return manifest, fmt.Errorf("%w: codebase.worktree_file_budget", ErrSourceBudget)
		}
		file := treeFile{path: string(name), object: fields[2], size: size}
		tracked[file.path] = true
		if err := ctx.Err(); err != nil {
			return manifest, err
		}
		fullPath := filepath.Join(checkout, filepath.FromSlash(file.path))
		info, err := os.Lstat(fullPath)
		if err != nil || !info.Mode().IsRegular() || info.Size() != file.size {
			return manifest, fmt.Errorf("%w: codebase.worktree_content_mismatch: %s", ErrSourceIntegrity, file.path)
		}
		f, err := os.Open(fullPath)
		if err != nil {
			return manifest, err
		}
		h := sha1.New()
		fmt.Fprintf(h, "blob %d\x00", file.size)
		n, copyErr := io.Copy(h, io.LimitReader(f, file.size+1))
		closeErr := f.Close()
		if copyErr != nil || closeErr != nil {
			return manifest, errors.Join(copyErr, closeErr)
		}
		if n != file.size || hex.EncodeToString(h.Sum(nil)) != file.object {
			return manifest, fmt.Errorf("%w: codebase.worktree_content_mismatch: %s", ErrSourceIntegrity, file.path)
		}
		manifest.Files++
		manifest.Bytes += file.size
		if manifest.Files > 200000 || manifest.Bytes > 4<<30 {
			return manifest, fmt.Errorf("%w: codebase.worktree_budget", ErrSourceBudget)
		}
	}
	if err := filepath.WalkDir(checkout, func(name string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		rel, err := filepath.Rel(checkout, name)
		if err != nil {
			return err
		}
		if rel == "." {
			return nil
		}
		rel = filepath.ToSlash(rel)
		if rel == ".git" {
			return nil
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("%w: codebase.worktree_extra_or_link: %s", ErrSourceIntegrity, rel)
		}
		if entry.IsDir() {
			return nil
		}
		if !entry.Type().IsRegular() || !tracked[rel] {
			return fmt.Errorf("%w: codebase.worktree_extra_or_link: %s", ErrSourceIntegrity, rel)
		}
		return nil
	}); err != nil {
		return manifest, err
	}
	return manifest, nil
}

// Check the .git indirection before invoking git -C on a checkout. A replaced
// gitfile could otherwise select an unrelated repository with executable local
// configuration before byte verification begins.
func verifyWorktreeGitBinding(checkout, mirror string) error {
	bad := fmt.Errorf("%w: codebase.worktree_identity_mismatch", ErrSourceIntegrity)
	readSmall := func(path string) (string, error) {
		info, err := os.Lstat(path)
		if err != nil || !info.Mode().IsRegular() || info.Size() > 4096 {
			return "", bad
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return "", err
		}
		return strings.TrimSpace(string(data)), nil
	}
	gitfile := filepath.Join(checkout, ".git")
	line, err := readSmall(gitfile)
	if err != nil || !strings.HasPrefix(line, "gitdir: ") {
		return bad
	}
	admin := strings.TrimPrefix(line, "gitdir: ")
	if !filepath.IsAbs(admin) {
		admin = filepath.Join(checkout, admin)
	}
	admin, err = filepath.EvalSymlinks(admin)
	if err != nil {
		return bad
	}
	if _, err := os.Lstat(filepath.Join(admin, "config.worktree")); err == nil {
		return bad
	} else if !errors.Is(err, os.ErrNotExist) {
		return bad
	}
	mirror, err = filepath.EvalSymlinks(mirror)
	if err != nil {
		return bad
	}
	rel, err := filepath.Rel(filepath.Join(mirror, "worktrees"), admin)
	if err != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) || strings.Contains(rel, string(filepath.Separator)) {
		return bad
	}
	common, err := readSmall(filepath.Join(admin, "commondir"))
	if err != nil {
		return bad
	}
	if !filepath.IsAbs(common) {
		common = filepath.Join(admin, common)
	}
	common, err = filepath.EvalSymlinks(common)
	if err != nil || !strings.EqualFold(common, mirror) {
		return bad
	}
	back, err := readSmall(filepath.Join(admin, "gitdir"))
	if err != nil {
		return bad
	}
	if !filepath.IsAbs(back) {
		back = filepath.Join(admin, back)
	}
	if !strings.EqualFold(filepath.Clean(back), filepath.Clean(gitfile)) {
		return bad
	}
	return nil
}
