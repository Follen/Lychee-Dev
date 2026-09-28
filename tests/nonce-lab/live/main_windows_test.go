//go:build windows && amd64

package main

import (
	"bytes"
	"golang.org/x/sys/windows"
	"os"
	"runtime"
	"testing"
	"unsafe"
)

func TestReadOnlySelf(t *testing.T) {
	h, e := windows.OpenProcess(windows.PROCESS_QUERY_INFORMATION|windows.PROCESS_VM_READ, false, uint32(os.Getpid()))
	if e != nil {
		t.Fatal(e)
	}
	defer windows.CloseHandle(h)
	source := []byte("read-only fixture 0123456789")
	dest := make([]byte, len(source))
	if !read(h, uintptr(unsafe.Pointer(&source[0])), dest) || !bytes.Equal(source, dest) {
		t.Fatal("read mismatch")
	}
	runtime.KeepAlive(source)
	if read(h, 1, dest) {
		t.Fatal("invalid address succeeded")
	}
	regions, _, e := enumerate(h)
	if e != nil || len(regions) == 0 {
		t.Fatal(e)
	}
}
func TestProtection(t *testing.T) {
	for _, p := range []uint32{1, 16, 0x104, 0} {
		if readable(p) {
			t.Fatal(p)
		}
	}
	for _, p := range []uint32{2, 4, 8, 32, 64, 128} {
		if !readable(p) {
			t.Fatal(p)
		}
	}
}
