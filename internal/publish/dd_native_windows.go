//go:build windows && amd64

package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"golang.org/x/sys/windows"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"unicode/utf8"
	"unsafe"
)

type ddNativeSigner struct {
	module                  windows.Handle
	file                    *os.File
	get, post, append, free uintptr
	mu                      sync.Mutex
}
type ddSignReply struct {
	count int
	value string
	err   error
}

var ddReplies sync.Map
var ddReplyID atomic.Uint64
var ddCallback = windows.NewCallbackCDecl(func(code uintptr, data *byte, token uintptr) uintptr {
	value, ok := ddReplies.Load(token)
	if !ok {
		return 0
	}
	reply := value.(*ddSignReply)
	reply.count++
	if data == nil {
		reply.err = capability("publish.dd_signer", fmt.Sprintf("native signature failed with code %d", int32(code)))
		return 0
	}
	bytes := make([]byte, 0, 1024)
	for i := 0; i <= 65536; i++ {
		b := *(*byte)(unsafe.Add(unsafe.Pointer(data), i))
		if b == 0 {
			if code != 1 {
				reply.err = capability("publish.dd_signer", fmt.Sprintf("native signature failed with code %d", int32(code)))
			} else if len(bytes) == 0 || !utf8.Valid(bytes) {
				reply.err = capability("publish.dd_signer", "invalid native signature encoding")
			} else {
				reply.value = string(bytes)
			}
			return 0
		}
		bytes = append(bytes, b)
	}
	reply.err = capability("publish.dd_signer", "native signature exceeds its bound")
	return 0
})

func openDDSigner(ctx context.Context, path string) (ddSigner, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !filepath.IsAbs(path) {
		return nil, invalid("$.sdk", "SDK path must be absolute")
	}
	absolute, err := filepath.EvalSymlinks(path)
	if err != nil {
		return nil, capability("publish.dd_sdk", "DD 100130 libnep.dll is unavailable")
	}
	f, err := openArtifact(absolute)
	if err != nil {
		return nil, capability("publish.dd_sdk", "cannot pin SDK bytes")
	}
	good := false
	defer func() {
		if !good {
			f.Close()
		}
	}()
	stat, err := f.Stat()
	if err != nil || stat.Size() == 0 || stat.Size() > 16<<20 {
		return nil, capability("publish.dd_sdk", "unsupported SDK file")
	}
	hash := sha256.New()
	if _, err = io.Copy(hash, f); err != nil {
		return nil, capability("publish.dd_sdk", "cannot hash SDK")
	}
	if hex.EncodeToString(hash.Sum(nil)) != ddNEPSHA256 {
		return nil, capability("publish.dd_sdk", "requires the pinned DD 100130 SDK")
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	module, err := windows.LoadLibraryEx(absolute, 0, windows.LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|windows.LOAD_LIBRARY_SEARCH_SYSTEM32)
	if err != nil {
		return nil, capability("publish.dd_sdk", "cannot load the pinned SDK")
	}
	name := make([]uint16, 32768)
	n, err := windows.GetModuleFileName(module, &name[0], uint32(len(name)))
	if err != nil || n == 0 || n >= uint32(len(name)) {
		windows.FreeLibrary(module)
		return nil, capability("publish.dd_sdk", "loaded SDK identity unavailable")
	}
	loaded, err := filepath.EvalSymlinks(windows.UTF16ToString(name[:n]))
	if err != nil || !strings.EqualFold(loaded, absolute) {
		windows.FreeLibrary(module)
		return nil, capability("publish.dd_sdk", "loaded SDK identity differs")
	}
	base := uintptr(module)
	good = true
	return &ddNativeSigner{module: module, file: f, get: base + 0x69b5, post: base + 0x549d, append: base + 0x37e2, free: base + 0x5c90}, nil
}
func (s *ddNativeSigner) Sign(ctx context.Context, url string, body []byte, extra [][2]string) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.module == 0 {
		return "", capability("publish.dd_signer", "signer is closed")
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if strings.ContainsRune(url, 0) {
		return "", invalid("$url", "URL contains NUL")
	}
	for _, pair := range extra {
		if strings.ContainsRune(pair[0], 0) || strings.ContainsRune(pair[1], 0) {
			return "", invalid("$headers", "header contains NUL")
		}
	}
	ptr, err := windows.BytePtrFromString(url)
	if err != nil {
		return "", invalid("$url", "invalid URL")
	}
	var list uintptr
	defer func() {
		if list != 0 {
			syscall.SyscallN(s.free, list)
		}
	}()
	for _, pair := range extra {
		key, value := []byte(pair[0]), []byte(pair[1])
		var kp, vp uintptr
		if len(key) > 0 {
			kp = uintptr(unsafe.Pointer(&key[0]))
		}
		if len(value) > 0 {
			vp = uintptr(unsafe.Pointer(&value[0]))
		}
		next, _, _ := syscall.SyscallN(s.append, list, kp, uintptr(len(key)), vp, uintptr(len(value)))
		runtime.KeepAlive(key)
		runtime.KeepAlive(value)
		if next == 0 {
			return "", capability("publish.dd_signer", "SDK rejected signature headers")
		}
		list = next
	}
	token := uintptr(ddReplyID.Add(1))
	reply := &ddSignReply{}
	ddReplies.Store(token, reply)
	defer ddReplies.Delete(token)
	if body == nil {
		syscall.SyscallN(s.get, uintptr(unsafe.Pointer(ptr)), list, ddCallback, token)
	} else {
		var bp uintptr
		if len(body) > 0 {
			bp = uintptr(unsafe.Pointer(&body[0]))
		}
		syscall.SyscallN(s.post, uintptr(unsafe.Pointer(ptr)), bp, uintptr(len(body)), list, ddCallback, token)
		runtime.KeepAlive(body)
	}
	runtime.KeepAlive(ptr)
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if reply.count != 1 {
		return "", capability("publish.dd_signer", "SDK did not return exactly one synchronous response")
	}
	if reply.err != nil {
		return "", reply.err
	}
	return reply.value, nil
}
func (s *ddNativeSigner) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.module == 0 {
		return nil
	}
	err := windows.FreeLibrary(s.module)
	s.module = 0
	return errors.Join(err, s.file.Close())
}
