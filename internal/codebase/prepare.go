package codebase

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// PrepareSource fetches one explicit ref into the new workspace. Fetched commit
// objects receive permanent pin refs; subsequent branch movement cannot change
// earlier reads. Empty reference selects the catalog's product branch.
func (b *Browser) PrepareSource(ctx context.Context, key, product, reference string) (selection.SourcePin, error) {
	var zero selection.SourcePin
	repo, err := LookupRepository(key)
	if err != nil {
		return zero, err
	}
	branch, ok := repo.Tracks[product]
	if !ok {
		return zero, errors.New("codebase.unknown_product")
	}
	if reference == "" {
		reference = "refs/heads/" + branch
	}
	if !objectID(reference) {
		if !strings.HasPrefix(reference, "refs/heads/") && !strings.HasPrefix(reference, "refs/tags/") {
			return zero, errors.New("codebase.explicit_ref_required")
		}
		if _, err := gitBytes(ctx, "", 1024, "check-ref-format", reference); err != nil {
			return zero, err
		}
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(b.store.Root(), "locks"), "source:"+repo.Key)
	if err != nil {
		return zero, err
	}
	defer lease.Close()
	destination := b.mirror(repo.Key)
	directory := destination
	created := false
	if info, err := os.Lstat(destination); errors.Is(err, os.ErrNotExist) {
		directory, err = os.MkdirTemp(filepath.Join(b.store.Root(), "mirrors"), ".source-")
		if err != nil {
			return zero, err
		}
		defer os.RemoveAll(directory) // Only this invocation's freshly allocated staging directory.
		if _, err := gitBytes(ctx, "", 4096, "init", "--bare", "--object-format=sha1", directory); err != nil {
			return zero, err
		}
		created = true
	} else if err != nil {
		return zero, err
	} else if !info.IsDir() {
		return zero, errors.New("codebase.invalid_mirror")
	}
	if _, err := gitBytes(ctx, directory, 4096, "-c", "fetch.fsckObjects=true", "fetch", "--quiet", "--no-tags", "--no-recurse-submodules", "--depth=1", repo.URL, reference); err != nil {
		return zero, err
	}
	resolved, err := gitBytes(ctx, directory, 128, "rev-parse", "--verify", "FETCH_HEAD^{commit}")
	if err != nil {
		return zero, err
	}
	commit := strings.TrimSpace(string(resolved))
	if !objectID(commit) || objectID(reference) && reference != commit {
		return zero, errors.New("codebase.commit_mismatch")
	}
	if _, err := gitBytes(ctx, directory, 1024, "update-ref", "refs/pins/"+commit, commit); err != nil {
		return zero, err
	}
	if created {
		if err := os.Rename(directory, destination); err != nil {
			return zero, err
		}
	}
	return selection.SourcePin{Repository: repo.Key, Product: product, RequestedRef: reference, ExactCommit: commit, ParserRevision: ParserRevision}, nil
}
