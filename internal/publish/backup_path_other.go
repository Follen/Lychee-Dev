//go:build !windows

package publish

import "path/filepath"

func backupResolvedPath(path string) (string, error) { return filepath.EvalSymlinks(path) }
