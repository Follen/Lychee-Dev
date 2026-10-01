package records

import (
	"context"
	"crypto/sha256"
	"io"
	"os"
	"path/filepath"
	"slices"

	"github.com/follenfang/lycheedev/internal/records/archive"
	"github.com/follenfang/lycheedev/internal/records/resource"
)

type localIndexSnapshot struct {
	digest [sha256.Size]byte
	size   int64
}
type localReadSession struct {
	dir      *os.Root
	files    []localDirectory
	indexes  map[string]*archive.Index
	observed map[string]localIndexSnapshot
	retained int64
	budget   *resource.Budget
}

func openLocalSession(ctx context.Context, root string, budget *resource.Budget) (*localReadSession, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	dir, err := os.OpenRoot(root)
	if err != nil {
		return nil, err
	}
	files, err := localDirectories(dir)
	if err != nil {
		dir.Close()
		return nil, err
	}
	return &localReadSession{dir: dir, files: files, indexes: map[string]*archive.Index{}, observed: map[string]localIndexSnapshot{}, budget: budget}, nil
}

func (s *localReadSession) open(ctx context.Context, key string, size int64) (encodedObject, error) {
	return openLocalObject(ctx, s.dir, s.files, key, size, func(entry localDirectory, key [16]byte) ([]archive.Span, error) {
		if cached := s.indexes[entry.name]; cached != nil {
			return cached.Find(ctx, key)
		}
		file, err := s.dir.Open(filepath.Join("Data", "data", entry.name))
		if err != nil {
			return nil, err
		}
		defer file.Close()
		stat, err := file.Stat()
		if err != nil {
			return nil, err
		}
		if !stat.Mode().IsRegular() || stat.Size() < 40 {
			return nil, archive.ErrIndexFormat
		}
		if stat.Size() > 64<<20 {
			return nil, archive.ErrIndexLimit
		}
		if err = s.budget.Charge(resource.Cost{RetainedBytes: stat.Size(), MetadataBytes: stat.Size(), DecodeWork: (stat.Size() + 65535) / 65536}); err != nil {
			return nil, err
		}
		index, err := archive.OpenIndex(ctx, file, stat.Size(), entry.bucket)
		if err != nil {
			return nil, err
		}
		snapshot := localIndexSnapshot{digest: index.SHA256(), size: stat.Size()}
		if previous, ok := s.observed[entry.name]; ok && previous != snapshot {
			return nil, ErrPinnedBuildChanged
		}
		s.observed[entry.name] = snapshot
		if stat.Size() > (64<<20)-s.retained {
			clear(s.indexes)
			s.retained = 0
		}
		s.indexes[entry.name] = index
		s.retained += stat.Size()
		return index.Find(ctx, key)
	})
}

func (s *localReadSession) check(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	current, err := localDirectories(s.dir)
	if err != nil {
		return err
	}
	if !slices.Equal(current, s.files) {
		return ErrPinnedBuildChanged
	}
	return nil
}

// Before publishing a successful query rehash the path, not an old handle or
// mtime. Generation reuse, replacement and same-size external edits all fail.
func (s *localReadSession) verify(ctx context.Context) error {
	if err := s.check(ctx); err != nil {
		return err
	}
	buffer := make([]byte, 64<<10)
	for name, snapshot := range s.observed {
		file, err := s.dir.Open(filepath.Join("Data", "data", name))
		if err != nil {
			return err
		}
		hash := sha256.New()
		var size int64
		for {
			if err = ctx.Err(); err != nil {
				break
			}
			var n int
			n, err = file.Read(buffer)
			if n > 0 {
				size += int64(n)
				hash.Write(buffer[:n])
				if size > snapshot.size {
					err = ErrPinnedBuildChanged
					break
				}
			}
			if err != nil {
				break
			}
		}
		closeErr := file.Close()
		if err != io.EOF {
			return err
		}
		if closeErr != nil {
			return closeErr
		}
		if size != snapshot.size || [sha256.Size]byte(hash.Sum(nil)) != snapshot.digest {
			return ErrPinnedBuildChanged
		}
	}
	return nil
}

func (s *localReadSession) close() { s.dir.Close(); s.indexes = nil; s.observed = nil }
