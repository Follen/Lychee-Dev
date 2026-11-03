//go:build windows

package publish

import (
	"golang.org/x/sys/windows"
	"runtime"
	"unsafe"
)

func ddBackupProtect(raw []byte, decrypt bool) ([]byte, error) {
	if len(raw) == 0 {
		return nil, failure("publish.dd_backup", "empty private backup grant")
	}
	entropy := []byte("LycheeDev.DD.Backup.CurrentUser.v1")
	in := windows.DataBlob{Size: uint32(len(raw)), Data: &raw[0]}
	ent := windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
	var out windows.DataBlob
	var err error
	if decrypt {
		err = windows.CryptUnprotectData(&in, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	} else {
		err = windows.CryptProtectData(&in, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	}
	runtime.KeepAlive(raw)
	runtime.KeepAlive(entropy)
	if err != nil {
		return nil, capability("publish.dd_backup", "original backup grant is unavailable to this Windows user")
	}
	defer windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(out.Data))))
	if out.Size > 1<<20 {
		return nil, failure("publish.dd_backup", "private backup grant exceeds limit")
	}
	return append([]byte(nil), unsafe.Slice(out.Data, int(out.Size))...), nil
}
