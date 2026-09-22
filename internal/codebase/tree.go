package codebase

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/selection"
)

type treeFile struct {
	path, object string
	size         int64
	asset        bool
}

const (
	maxDocumentEntries = 100000
	maxAssetEntries    = 200000
	maxTreeReadBytes   = 1 << 30
)

// sourceTree lists the analyzable documents and assets of one pinned commit.
// Only regular blobs with safe relative paths are accepted; documents above the
// document byte limit fail the listing, while oversized assets keep a
// metadata-only identity without ever being read.
func (b *Browser) sourceTree(ctx context.Context, pin selection.SourcePin) ([]treeFile, error) {
	repo, err := LookupRepository(pin.Repository)
	if err != nil {
		return nil, err
	}
	if _, ok := repo.Tracks[pin.Product]; !ok || !objectID(pin.ExactCommit) || pin.ParserRevision != ParserRevision {
		return nil, errors.New("codebase.invalid_source_pin")
	}
	data, err := gitBytes(ctx, b.mirror(repo.Key), 32<<20, "ls-tree", "-r", "-l", "-z", "--full-tree", pin.ExactCommit)
	if err != nil {
		return nil, err
	}
	files := []treeFile{}
	var documents, assets int
	var readTotal int64
	for _, entry := range bytes.Split(data, []byte{0}) {
		if len(entry) == 0 {
			continue
		}
		header, name, ok := bytes.Cut(entry, []byte{'\t'})
		fields := strings.Fields(string(header))
		if !ok || len(fields) != 4 {
			return nil, errors.New("codebase.invalid_tree_record")
		}
		if fields[0] != "100644" && fields[0] != "100755" {
			continue
		}
		extension := strings.ToLower(path.Ext(string(name)))
		document := extension == ".lua" || extension == ".xml" || extension == ".toc"
		asset := isAssetExtension(extension)
		if !document && !asset {
			continue
		}
		size, err := strconv.ParseInt(fields[3], 10, 64)
		if err != nil || size < 0 || fields[1] != "blob" || !objectID(fields[2]) || !sourcePath(string(name)) {
			return nil, errors.New("codebase.invalid_document_object")
		}
		if document && size > maxSourceBytes {
			return nil, errors.New("codebase.invalid_document_object")
		}
		file := treeFile{path: string(name), object: fields[2], size: size, asset: asset}
		if asset {
			assets++
			if assets > maxAssetEntries {
				return nil, errors.New("codebase.tree_budget")
			}
		} else {
			documents++
			if documents > maxDocumentEntries {
				return nil, errors.New("codebase.tree_budget")
			}
		}
		if treeNeedsRead(file) {
			readTotal += size
			if readTotal > maxTreeReadBytes {
				return nil, errors.New("codebase.tree_budget")
			}
		}
		files = append(files, file)
	}
	return files, nil
}

// treeNeedsRead reports whether the indexer reads the file's bytes. Assets
// above the asset byte limit stay metadata-only rows.
func treeNeedsRead(file treeFile) bool {
	return !file.asset || file.size <= maxAssetBytes
}

// directoryTree lists fixture-directory documents and assets with the same
// admission rules as a pinned Git tree. Symbolic links are never followed.
func directoryTree(root string) ([]treeFile, error) {
	files := []treeFile{}
	var documents, assets int
	var readTotal int64
	err := filepath.WalkDir(root, func(name string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() || entry.Type()&os.ModeSymlink != 0 {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return nil
		}
		relative, err := filepath.Rel(root, name)
		if err != nil {
			return err
		}
		relative = filepath.ToSlash(relative)
		extension := strings.ToLower(path.Ext(relative))
		document := extension == ".lua" || extension == ".xml" || extension == ".toc"
		asset := isAssetExtension(extension)
		if !document && !asset {
			return nil
		}
		size := info.Size()
		if size < 0 || !sourcePath(relative) {
			return errors.New("codebase.invalid_document_object")
		}
		if document && size > maxSourceBytes {
			return errors.New("codebase.invalid_document_object")
		}
		file := treeFile{path: relative, size: size, asset: asset}
		if asset {
			assets++
			if assets > maxAssetEntries {
				return errors.New("codebase.tree_budget")
			}
		} else {
			documents++
			if documents > maxDocumentEntries {
				return errors.New("codebase.tree_budget")
			}
		}
		if treeNeedsRead(file) {
			readTotal += size
			if readTotal > maxTreeReadBytes {
				return errors.New("codebase.tree_budget")
			}
		}
		files = append(files, file)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return files, nil
}

// visitDirectory reads fixture files in listing order with per-file budgets.
func visitDirectory(ctx context.Context, root string, files []treeFile, visit func(treeFile, []byte) error) error {
	for _, file := range files {
		if err := ctx.Err(); err != nil {
			return err
		}
		if !treeNeedsRead(file) {
			continue
		}
		handle, err := os.Open(filepath.Join(root, filepath.FromSlash(file.path)))
		if err != nil {
			return err
		}
		limit := maxSourceBytes
		if file.asset {
			limit = maxAssetBytes
		}
		data, readErr := io.ReadAll(io.LimitReader(handle, int64(limit)+1))
		closeErr := handle.Close()
		if readErr != nil || closeErr != nil {
			return errors.Join(readErr, closeErr)
		}
		if int64(len(data)) != file.size || len(data) > limit {
			return errors.New("codebase.invalid_document_object")
		}
		if err := visit(file, data); err != nil {
			return err
		}
	}
	return nil
}

// One Git process supplies an entire index, avoiding three subprocesses per
// source file. Each object is independently bounded and verified before use.
func (b *Browser) visitDocuments(ctx context.Context, pin selection.SourcePin, files []treeFile, visit func(treeFile, []byte) error) error {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	cmd := gitCommand(ctx, b.mirror(pin.Repository), "cat-file", "--batch")
	input, err := cmd.StdinPipe()
	if err != nil {
		return err
	}
	output, err := cmd.StdoutPipe()
	if err != nil {
		input.Close()
		return err
	}
	log := &boundedOutput{limit: 32768, cancel: cancel}
	cmd.Stderr = log
	if err := cmd.Start(); err != nil {
		input.Close()
		output.Close()
		return err
	}
	reader := bufio.NewReaderSize(output, 65536)
	consume := func() error {
		for _, file := range files {
			if err := ctx.Err(); err != nil {
				return err
			}
			if !treeNeedsRead(file) {
				continue
			}
			if _, err := io.WriteString(input, file.object+"\n"); err != nil {
				return err
			}
			header, err := reader.ReadSlice('\n')
			if err != nil {
				return err
			}
			if string(header) != fmt.Sprintf("%s blob %d\n", file.object, file.size) {
				return errors.New("codebase.batch_identity_mismatch")
			}
			data := make([]byte, int(file.size)+1)
			if _, err := io.ReadFull(reader, data); err != nil {
				return err
			}
			if data[len(data)-1] != '\n' {
				return errors.New("codebase.batch_framing")
			}
			data = data[:len(data)-1]
			digest := sha1.New()
			fmt.Fprintf(digest, "blob %d\x00", len(data))
			digest.Write(data)
			if hex.EncodeToString(digest.Sum(nil)) != file.object {
				return errors.New("codebase.git_blob_integrity")
			}
			if err := visit(file, data); err != nil {
				return err
			}
		}
		return nil
	}
	readErr := consume()
	input.Close()
	if readErr != nil {
		cancel()
		output.Close()
	}
	waitErr := cmd.Wait()
	if readErr != nil {
		return readErr
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if waitErr != nil {
		return fmt.Errorf("codebase.batch_failed: %w: %s", waitErr, log.String())
	}
	return nil
}
