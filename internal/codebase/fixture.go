package codebase

import (
	"errors"
	"fmt"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/selection"
)

// SyntheticCommitID is the deterministic commit identity of a fixture source
// directory. The rule is frozen for identifier stability and matches the
// legacy fixture-index identity exactly:
//
//	x = 1469598103934665603
//	for each byte b of the absolute path: x = (x XOR b) * 1099511628211 (mod 2^64)
//	result = x formatted as 40-character zero-padded lowercase hex
//
// The offset basis deliberately keeps the legacy value (which is one digit
// short of the standard FNV-1a basis) so synthetic IDs derived by earlier
// tools for the same absolute path stay identical. The rule must never change
// without a new ParserRevision: indexes and pins key on the result.
func SyntheticCommitID(absolutePath string) string {
	var x uint64 = 1469598103934665603
	for i := 0; i < len(absolutePath); i++ {
		x ^= uint64(absolutePath[i])
		x *= 1099511628211
	}
	return fmt.Sprintf("%040x", x)
}

// FixtureSourcePin derives the immutable pin of a fixture directory. The
// repository and product keep catalog identity; the commit is synthetic
// because fixture trees are not Git repositories.
func FixtureSourcePin(repository, product, sourcePath string) (selection.SourcePin, error) {
	var zero selection.SourcePin
	repo, err := LookupRepository(repository)
	if err != nil {
		return zero, err
	}
	if _, ok := repo.Tracks[product]; !ok {
		return zero, errors.New("codebase.unknown_product")
	}
	absolute, err := filepath.Abs(sourcePath)
	if err != nil {
		return zero, err
	}
	return selection.SourcePin{
		Repository:     repo.Key,
		Product:        product,
		RequestedRef:   "source-path",
		ExactCommit:    SyntheticCommitID(absolute),
		ParserRevision: ParserRevision,
	}, nil
}
