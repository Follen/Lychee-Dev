package records

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

var ErrExportPath = errors.New("records.export_path")
var ErrExportConflict = errors.New("records.export_conflict")

// The parent handle stays open across preparation and publication. Only a
// complete sibling file is linked or renamed into place; never truncate a
// caller's existing file, or follow a destination symlink.
type exportDestination struct {
	root      *os.Root
	path      string
	name      string
	overwrite bool
}

func openExportDestination(output string, overwrite bool) (*exportDestination, error) {
	if output == "" || strings.HasSuffix(output, "/") || strings.HasSuffix(output, string(filepath.Separator)) {
		return nil, ErrExportPath
	}
	abs, err := filepath.Abs(output)
	if err != nil || abs == filepath.Dir(abs) {
		return nil, ErrExportPath
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(abs))
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrExportPath, err)
	}
	root, err := os.OpenRoot(parent)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrExportPath, err)
	}
	dst := &exportDestination{root: root, path: filepath.Join(parent, filepath.Base(abs)), name: filepath.Base(abs), overwrite: overwrite}
	if err := dst.check(); err != nil {
		root.Close()
		return nil, err
	}
	return dst, nil
}

func (d *exportDestination) Close() error { return d.root.Close() }

func (d *exportDestination) check() error {
	info, err := d.root.Lstat(d.name)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("%w: %v", ErrExportPath, err)
	}
	if !info.Mode().IsRegular() || !d.overwrite {
		return fmt.Errorf("%w: %s", ErrExportConflict, d.path)
	}
	return nil
}

func (d *exportDestination) publish(ctx context.Context, raw []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := d.check(); err != nil {
		return err
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return err
	}
	stage := ".lycheedev-export-" + hex.EncodeToString(random[:])
	file, err := d.root.OpenFile(stage, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer d.root.Remove(stage)
	defer file.Close()
	for start := 0; start < len(raw); {
		if err := ctx.Err(); err != nil {
			return err
		}
		end := min(start+64<<10, len(raw))
		if _, err := file.Write(raw[start:end]); err != nil {
			return err
		}
		start = end
	}
	if err := file.Sync(); err != nil {
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := d.check(); err != nil {
		return err
	}
	if d.overwrite {
		err = d.root.Rename(stage, d.name)
	} else {
		err = d.root.Link(stage, d.name)
	}
	if errors.Is(err, os.ErrExist) {
		return fmt.Errorf("%w: %s", ErrExportConflict, d.path)
	}
	return err
}
