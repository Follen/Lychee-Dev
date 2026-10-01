package luals

import (
	"os"
	"path/filepath"
)

// The retained log/meta/definitions footprint is independent of LSP wire I/O.
func checkSessionArtifacts(dir string) error {
	nodes := 0
	var total int64
	return filepath.WalkDir(dir, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		nodes++
		if nodes > 10000 {
			return ErrBudget
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return ErrBudget
		}
		if !entry.IsDir() {
			info, err := entry.Info()
			if err != nil {
				return err
			}
			if !info.Mode().IsRegular() {
				return ErrBudget
			}
			total += info.Size()
			if total > 64<<20 {
				return ErrBudget
			}
		}
		return nil
	})
}
