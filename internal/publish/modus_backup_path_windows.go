//go:build windows

package publish

import "golang.org/x/sys/windows"

func modusBackupNoLink(path string) error {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return err
	}
	attr, err := windows.GetFileAttributes(p)
	if err != nil {
		return err
	}
	if attr&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return invalid("$.manifest_file", "backup paths cannot traverse symbolic links or junctions")
	}
	return nil
}
