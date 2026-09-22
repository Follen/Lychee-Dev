// Package codebase reads immutable source objects without executing repository
// code, checking out worktrees, or borrowing legacy tool state.
package codebase

import (
	"bytes"
	"context"
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const ParserRevision = "source-v1"
const maxSourceBytes = 16 << 20

type Browser struct{ store *vault.Store }

func OpenBrowser(store *vault.Store) *Browser { return &Browser{store: store} }

type SpanQuery struct {
	Path      string `json:"path"`
	FirstLine int    `json:"firstLine"`
	LineCount int    `json:"lineCount"`
}

type SourceExcerpt struct {
	Repository string        `json:"repository"`
	Commit     string        `json:"commit"`
	Path       string        `json:"path"`
	FirstLine  int           `json:"firstLine"`
	LastLine   int           `json:"lastLine"`
	TotalLines int           `json:"totalLines"`
	Text       string        `json:"text"`
	Blob       vault.BlobRef `json:"blob"`
}

// ReadSpan reads exactly the pinned commit. It never fetches, resolves a moving
// branch, follows a symlink, or runs textconv/filter drivers. The original full
// file is stored once; excerpts retain original CRLF and final-newline bytes.
func (b *Browser) ReadSpan(ctx context.Context, pin selection.SourcePin, query SpanQuery) (SourceExcerpt, error) {
	var result SourceExcerpt
	repo, err := LookupRepository(pin.Repository)
	if err != nil {
		return result, err
	}
	if _, ok := repo.Tracks[pin.Product]; !ok || !objectID(pin.ExactCommit) || pin.ParserRevision != ParserRevision {
		return result, errors.New("codebase.invalid_source_pin")
	}
	if !sourcePath(query.Path) || query.FirstLine < 1 || query.LineCount < 1 || query.LineCount > 2000 {
		return result, errors.New("codebase.invalid_span")
	}
	directory := b.mirror(repo.Key)
	if _, err := os.Stat(directory); err != nil {
		return result, fmt.Errorf("codebase.source_not_ready: %w", err)
	}
	commit, err := gitBytes(ctx, directory, 128, "rev-parse", "--verify", pin.ExactCommit+"^{commit}")
	if err != nil {
		return result, err
	}
	if strings.TrimSpace(string(commit)) != pin.ExactCommit {
		return result, errors.New("codebase.commit_mismatch")
	}
	entry, err := gitBytes(ctx, directory, 8192, "ls-tree", "-z", "--full-tree", pin.ExactCommit, "--", ":(literal)"+query.Path)
	if err != nil {
		return result, err
	}
	header, name, ok := bytes.Cut(entry, []byte{'\t'})
	fields := strings.Fields(string(header))
	if !ok || len(fields) != 3 || fields[1] != "blob" || (fields[0] != "100644" && fields[0] != "100755") || string(name) != query.Path+"\x00" || !objectID(fields[2]) {
		return result, errors.New("codebase.not_regular_source_file")
	}
	sizeText, err := gitBytes(ctx, directory, 128, "cat-file", "-s", fields[2])
	if err != nil {
		return result, err
	}
	size, err := strconv.ParseInt(strings.TrimSpace(string(sizeText)), 10, 64)
	if err != nil || size < 0 || size > maxSourceBytes {
		return result, errors.New("codebase.source_byte_limit")
	}
	data, err := gitBytes(ctx, directory, int(size)+1, "cat-file", "blob", fields[2])
	if err != nil {
		return result, err
	}
	if int64(len(data)) != size || !utf8.Valid(data) || bytes.IndexByte(data, 0) >= 0 {
		return result, errors.New("codebase.source_encoding_or_size")
	}
	// Git's object identity is SHA-1 for these repositories. Check its framed
	// bytes as well as preserving an independent SHA-256 evidence identity.
	hash := sha1.New()
	fmt.Fprintf(hash, "blob %d\x00", len(data))
	hash.Write(data)
	if hex.EncodeToString(hash.Sum(nil)) != fields[2] {
		return result, errors.New("codebase.git_blob_integrity")
	}
	starts := []int{0}
	for i, c := range data {
		if c == '\n' && i+1 < len(data) {
			starts = append(starts, i+1)
		}
	}
	if len(data) == 0 {
		starts = nil
	}
	if query.FirstLine > len(starts) {
		return result, errors.New("codebase.line_out_of_range")
	}
	last := min(len(starts), query.FirstLine+query.LineCount-1)
	end := len(data)
	if last < len(starts) {
		end = starts[last]
	}
	blob, err := b.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(data), MaxBytes: maxSourceBytes})
	if err != nil {
		return result, err
	}
	return SourceExcerpt{Repository: repo.Key, Commit: pin.ExactCommit, Path: query.Path, FirstLine: query.FirstLine, LastLine: last, TotalLines: len(starts), Text: string(data[starts[query.FirstLine-1]:end]), Blob: blob}, nil
}

func (b *Browser) mirror(key string) string {
	return filepath.Join(b.store.Root(), "mirrors", key+".git")
}
func objectID(value string) bool {
	if len(value) != 40 {
		return false
	}
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
func sourcePath(value string) bool {
	if value == "" || len(value) > 4096 || !utf8.ValidString(value) || path.IsAbs(value) || path.Clean(value) != value || value == "." || value == ".." || strings.HasPrefix(value, "../") || strings.ContainsAny(value, "\\:\x00\r\n") {
		return false
	}
	return true
}
