package codebase

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

type AddonInput struct{ Root, Manifest string }
type ValidationIssue struct {
	Severity  string `json:"severity"`
	Code      string `json:"code"`
	Path      string `json:"path"`
	Line      int    `json:"line"`
	Reference string `json:"reference,omitempty"`
	Message   string `json:"message"`
}
type LoadStep struct {
	From     string `json:"from"`
	Line     int    `json:"line"`
	Path     string `json:"path"`
	Kind     string `json:"kind"`
	Repeated bool   `json:"repeated"`
	Cycle    bool   `json:"cycle"`
}
type LoadedDocument struct {
	// Blob identifies the bytes; Archived says whether they were persisted.
	Path     string        `json:"path"`
	Blob     vault.BlobRef `json:"blob"`
	Archived bool          `json:"archived"`
	Facts    DocumentFacts `json:"facts"`
}
type LoadAssessment struct {
	Root                  string            `json:"root"`
	Manifest              string            `json:"manifest"`
	Documents             []LoadedDocument  `json:"documents"`
	Steps                 []LoadStep        `json:"steps"`
	Issues                []ValidationIssue `json:"issues"`
	LoadValid             bool              `json:"loadValid"`
	CompatibilityComplete bool              `json:"compatibilityComplete"`
}
type Checker struct{ store *vault.Store }

func OpenChecker(store *vault.Store) *Checker { return &Checker{store: store} }

// InspectAddon constructs an ordered, bounded local load graph and archives
// exactly the bytes analyzed. Compatibility against a pinned game source is a
// separate stage; LoadValid never claims that APIs or protected calls are safe.
func (c *Checker) InspectAddon(ctx context.Context, input AddonInput) (LoadAssessment, error) {
	if c.store == nil {
		return LoadAssessment{}, errors.New("codebase.archive_required")
	}
	return inspectLoad(ctx, input, c.store)
}

// InspectLocalLoad checks the same bounded load graph without creating a
// workspace or archiving content. Document digests are observations only;
// Archived remains false and compatibility is not asserted.
func InspectLocalLoad(ctx context.Context, input AddonInput) (LoadAssessment, error) {
	return inspectLoad(ctx, input, nil)
}

func inspectLoad(ctx context.Context, input AddonInput, store *vault.Store) (LoadAssessment, error) {
	result := LoadAssessment{Documents: []LoadedDocument{}, Steps: []LoadStep{}, Issues: []ValidationIssue{}, LoadValid: true}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	abs, err := filepath.Abs(input.Root)
	if err != nil {
		return result, err
	}
	root, err := os.OpenRoot(abs)
	if err != nil {
		return result, err
	}
	defer root.Close()
	result.Root = abs
	w := closureWalk{ctx: ctx, root: root, store: store, result: &result, seen: map[string]bool{}, active: map[string]bool{}, directories: map[string][]os.DirEntry{}}
	manifest, err := w.resolve("", input.Manifest)
	if err != nil {
		if ctx.Err() != nil {
			return result, ctx.Err()
		}
		w.issue("error", resolveIssueCode(err), "", 0, input.Manifest, err.Error())
		return result, nil
	}
	result.Manifest = manifest
	if !strings.EqualFold(path.Ext(manifest), ".toc") {
		return result, errors.New("codebase.manifest_must_be_toc")
	}
	err = w.load(manifest, "", 0, 0, "toc", true)
	return result, err
}

type closureWalk struct {
	ctx            context.Context
	root           *os.Root
	store          *vault.Store
	result         *LoadAssessment
	seen, active   map[string]bool
	directories    map[string][]os.DirEntry
	totalBytes     int64
	visitedEntries int
	references     int
}

func (w *closureWalk) issue(severity, code, file string, line int, reference, message string) {
	w.result.Issues = append(w.result.Issues, ValidationIssue{Severity: severity, Code: code, Path: file, Line: line, Reference: reference, Message: message})
	if severity == "error" {
		w.result.LoadValid = false
	}
}

// resolveFailure classifies load-path resolution problems so callers can report
// a missing file differently from a path that escapes the AddOn root.
type resolveFailure struct {
	kind string // "escape", "missing", or "unreadable"
	err  error
}

func (e *resolveFailure) Error() string {
	if e.err == nil {
		return e.kind
	}
	return e.err.Error()
}

func resolveIssueCode(err error) string {
	var failure *resolveFailure
	if errors.As(err, &failure) {
		switch failure.kind {
		case "escape":
			return "load_path_escape"
		case "missing":
			return "load_file_missing"
		}
	}
	return "load_reference"
}

func (w *closureWalk) resolve(from, reference string) (string, error) {
	if err := w.ctx.Err(); err != nil {
		return "", err
	}
	reference = strings.ReplaceAll(strings.TrimSpace(reference), "\\", "/")
	if reference == "" {
		return "", &resolveFailure{kind: "missing", err: os.ErrNotExist}
	}
	if !utf8.ValidString(reference) || path.IsAbs(reference) || strings.ContainsAny(reference, ":\x00\r\n") {
		return "", &resolveFailure{kind: "escape"}
	}
	name := path.Clean(path.Join(path.Dir(from), reference))
	if name == "." || name == ".." || strings.HasPrefix(name, "../") {
		return "", &resolveFailure{kind: "escape"}
	}
	current := "."
	for _, component := range strings.Split(name, "/") {
		entries, ok := w.directories[current]
		if !ok {
			dir, err := w.root.Open(filepath.FromSlash(current))
			if err != nil {
				return "", &resolveFailure{kind: "unreadable", err: err}
			}
			entries, err = dir.ReadDir(50001)
			dir.Close()
			if err != nil && err != io.EOF {
				return "", &resolveFailure{kind: "unreadable", err: err}
			}
			w.visitedEntries += len(entries)
			if len(entries) > 50000 || w.visitedEntries > 100000 {
				return "", &resolveFailure{kind: "unreadable", err: errors.New("directory entry budget exceeded")}
			}
			w.directories[current] = entries
		}
		matched := ""
		for _, entry := range entries {
			if strings.EqualFold(entry.Name(), component) {
				if matched != "" {
					return "", &resolveFailure{kind: "unreadable", err: errors.New("ambiguous case-insensitive load path")}
				}
				matched = entry.Name()
			}
		}
		if matched == "" {
			return "", &resolveFailure{kind: "missing", err: fmt.Errorf("load file missing: %s", name)}
		}
		current = path.Join(current, matched)
	}
	return current, nil
}

func (w *closureWalk) load(name, from string, line, depth int, kind string, manifest bool) error {
	if err := w.ctx.Err(); err != nil {
		return err
	}
	if depth > 64 || len(w.result.Steps) >= 20000 || len(w.result.Documents) >= 10000 {
		return errors.New("codebase.closure_budget")
	}
	key := strings.ToLower(name)
	repeated, cycle := w.seen[key], w.active[key]
	w.result.Steps = append(w.result.Steps, LoadStep{From: from, Line: line, Path: name, Kind: kind, Repeated: repeated, Cycle: cycle})
	if cycle {
		w.issue("error", "load_cycle", from, line, name, "recursive XML load reference")
		return nil
	}
	if repeated {
		w.issue("warning", "duplicate_load", from, line, name, "document already analyzed; repeated edge retained")
		return nil
	}
	extension := strings.ToLower(path.Ext(name))
	if !manifest && extension != ".lua" && extension != ".xml" {
		w.issue("error", "unsupported_load", from, line, name, "only Lua/XML may be loaded by this reference")
		return nil
	}
	info, err := w.root.Stat(filepath.FromSlash(name))
	if err != nil || !info.Mode().IsRegular() {
		w.issue("error", "load_unreadable", from, line, name, "target is unavailable or not a regular file")
		return nil
	}
	f, err := w.root.Open(filepath.FromSlash(name))
	if err != nil {
		w.issue("error", "load_unreadable", from, line, name, err.Error())
		return nil
	}
	data, readErr := io.ReadAll(io.LimitReader(f, maxSourceBytes+1))
	closeErr := f.Close()
	if readErr != nil || closeErr != nil {
		w.issue("error", "load_unreadable", from, line, name, fmt.Sprint(errors.Join(readErr, closeErr)))
		return nil
	}
	w.totalBytes += int64(len(data))
	if len(data) > maxSourceBytes || w.totalBytes > 64<<20 {
		return errors.New("codebase.closure_byte_budget")
	}
	facts, err := AnalyzeDocument(w.ctx, name, data)
	if err != nil {
		w.issue("error", "source_invalid", name, 0, "", err.Error())
		return nil
	}
	digest := sha256.Sum256(data)
	blob := vault.BlobRef{SHA256: hex.EncodeToString(digest[:]), Bytes: int64(len(data))}
	if w.store != nil {
		blob, err = w.store.PublishBlob(w.ctx, vault.BlobInput{Reader: bytes.NewReader(data), MaxBytes: maxSourceBytes, ExpectedSHA256: blob.SHA256})
		if err != nil {
			return err
		}
	}
	w.seen[key] = true
	w.active[key] = true
	defer delete(w.active, key)
	w.result.Documents = append(w.result.Documents, LoadedDocument{Path: name, Blob: blob, Archived: w.store != nil, Facts: facts})
	for _, note := range facts.Diagnostics {
		w.issue("error", "syntax", name, note.Line, "", note.Message)
	}
	for _, load := range facts.Loads {
		w.references++
		if w.references > 20000 {
			return errors.New("codebase.closure_reference_budget")
		}
		child, err := w.resolve(name, load.Path)
		if err != nil {
			if w.ctx.Err() != nil {
				return w.ctx.Err()
			}
			w.issue("error", resolveIssueCode(err), name, load.Line, load.Path, err.Error())
			continue
		}
		if err := w.load(child, name, load.Line, depth+1, load.Kind, false); err != nil {
			return err
		}
	}
	return nil
}
