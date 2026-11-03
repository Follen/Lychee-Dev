//go:build !windows

package publish

import (
	"os"
)

func modusBackupNoLink(path string) error {
	stat, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if stat.Mode()&os.ModeSymlink != 0 {
		return invalid("$.manifest_file", "backup paths cannot traverse symbolic links")
	}
	return nil
}
