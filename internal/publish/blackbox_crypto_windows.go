//go:build windows

package publish

import (
	"golang.org/x/sys/windows"
	"runtime"
	"unsafe"
)

func bbProtect(raw []byte, decrypt bool) ([]byte, error) {
	if len(raw) == 0 {
		return nil, failure("publish.blackbox_storage", "empty encrypted state")
	}
	entropy := []byte("LycheeDev.Heybox.CurrentUser.v1")
	input := windows.DataBlob{Size: uint32(len(raw)), Data: &raw[0]}
	ent := windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
	var out windows.DataBlob
	var err error
	if decrypt {
		err = windows.CryptUnprotectData(&input, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	} else {
		err = windows.CryptProtectData(&input, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out)
	}
	runtime.KeepAlive(raw)
	runtime.KeepAlive(entropy)
	if err != nil {
		return nil, capability("publish.blackbox_storage", "encrypted state is unavailable to this Windows user")
	}
	defer windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(out.Data))))
	if out.Size > 8<<20 {
		return nil, failure("publish.blackbox_storage", "encrypted state exceeds limit")
	}
	return append([]byte(nil), unsafe.Slice(out.Data, int(out.Size))...), nil
}
