package publish

import (
	"os"
	"path/filepath"
	"strings"
)

// backupPathOutside compares absolute paths after their filesystem aliases
// have been resolved. Separate Windows volumes are outside each other.
func backupPathOutside(root, destination string) bool {
	if !strings.EqualFold(filepath.VolumeName(root), filepath.VolumeName(destination)) {
		return true
	}
	rel, err := filepath.Rel(root, destination)
	return err == nil && (rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)))
}

// backupOutputWithin resolves the existing game root and the destination's
// nearest existing ancestor, including Windows directory junctions.
func backupOutputWithin(root, destination string) (bool, error) {
	resolvedRoot, err := backupResolvedPath(root)
	if err != nil {
		return false, err
	}
	ancestor := destination
	for {
		if _, e := os.Lstat(ancestor); e == nil {
			break
		} else if !os.IsNotExist(e) {
			return false, e
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return false, os.ErrNotExist
		}
		ancestor = parent
	}
	resolvedAncestor, err := backupResolvedPath(ancestor)
	if err != nil {
		return false, err
	}
	suffix, err := filepath.Rel(ancestor, destination)
	if err != nil {
		return false, err
	}
	return !backupPathOutside(resolvedRoot, filepath.Join(resolvedAncestor, suffix)), nil
}
