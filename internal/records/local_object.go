// SPDX-License-Identifier: MIT
// CASC archive handling adapted from wowdata; see LICENSE.
package records

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/records/archive"
)

// LocalObject owns one read-only archive handle and a bounded BLTE section.
// Close it after all readers finish. The source must not be modified concurrently;
// per-chunk checks in container.Ranges detect payload corruption during reads.
type LocalObject struct {
	*io.SectionReader
	file  *os.File
	Index string
	Span  archive.Span
}

var ErrObjectUnavailable = errors.New("records.local_object_unavailable")

func (o *LocalObject) Close() error { return o.file.Close() }

// OpenLocalObject resolves a full encoding identity and exact BLTE extent from
// the newest generation of each index bucket. It never silently falls back to a
// damaged older generation or to CDN. No index entries persist in a global cache.
func OpenLocalObject(ctx context.Context, root, encodingKey string, encodedBytes int64) (*LocalObject, error) {
	if !metadataKey(encodingKey) || encodedBytes < 8 || encodedBytes > 1<<40 {
		return nil, ErrMetadataFormat
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	dir, err := os.OpenRoot(root)
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	files, err := localDirectories(dir)
	if err != nil {
		return nil, err
	}
	decoded, _ := hex.DecodeString(encodingKey)
	var key [16]byte
	copy(key[:], decoded)
	var candidateError error
	for _, index := range files {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		f, err := dir.Open(filepath.Join("Data", "data", index.name))
		if err != nil {
			return nil, err
		}
		stat, err := f.Stat()
		if err != nil {
			f.Close()
			return nil, err
		}
		if !stat.Mode().IsRegular() {
			f.Close()
			return nil, ErrMetadataFormat
		}
		spans, err := archive.FindIndexSpans(ctx, f, stat.Size(), key, index.bucket)
		f.Close()
		if err != nil {
			return nil, fmt.Errorf("index %s: %w", index.name, err)
		}
		for _, span := range spans {
			if span.Bytes != encodedBytes+30 {
				candidateError = archive.ErrIndexIntegrity
				continue
			}
			file, err := dir.Open(filepath.Join("Data", "data", span.Filename()))
			if err != nil {
				return nil, err
			}
			info, err := file.Stat()
			if err != nil {
				file.Close()
				return nil, err
			}
			if !info.Mode().IsRegular() {
				file.Close()
				return nil, ErrMetadataFormat
			}
			section, err := archive.OpenEncodedSpan(ctx, file, info.Size(), span, key, encodedBytes)
			if err != nil {
				file.Close()
				if errors.Is(err, archive.ErrIndexIntegrity) {
					candidateError = err
					continue
				}
				return nil, err
			}
			return &LocalObject{SectionReader: section, file: file, Index: index.name, Span: span}, nil
		}
	}
	if candidateError != nil {
		return nil, candidateError
	}
	return nil, fmt.Errorf("%w: %s", ErrObjectUnavailable, encodingKey)
}

type localDirectory struct {
	name       string
	bucket     byte
	generation uint32
}

func localDirectories(root *os.Root) ([]localDirectory, error) {
	dir, err := root.Open(filepath.Join("Data", "data"))
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	entries, err := dir.ReadDir(4097)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, err
	}
	if len(entries) > 4096 {
		return nil, ErrMetadataLimit
	}
	selected := make(map[byte]localDirectory)
	for _, entry := range entries {
		name := entry.Name()
		if len(name) != 14 || !strings.EqualFold(name[10:], ".idx") || entry.IsDir() {
			continue
		}
		bucket, err := strconv.ParseUint(name[:2], 16, 8)
		if err != nil || bucket > 15 {
			continue
		}
		generation, err := strconv.ParseUint(name[2:10], 16, 32)
		if err != nil {
			continue
		}
		next := localDirectory{name: name, bucket: byte(bucket), generation: uint32(generation)}
		old, exists := selected[next.bucket]
		if exists && old.generation == next.generation {
			return nil, ErrBuildAmbiguous
		}
		if !exists || next.generation > old.generation {
			selected[next.bucket] = next
		}
	}
	result := make([]localDirectory, 0, len(selected))
	for _, entry := range selected {
		result = append(result, entry)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].bucket < result[j].bucket })
	return result, nil
}
