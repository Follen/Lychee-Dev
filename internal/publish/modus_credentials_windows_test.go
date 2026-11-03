//go:build windows

package publish

import (
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestModusCreatorUsesCurrentUserEntropyAndDoesNotFallbackOnDamage(t *testing.T) {
	dir := t.TempDir()
	raw := []byte("test-only-token")
	entropy := []byte("ModUs.Creator.TokenStore.v1")
	in := windows.DataBlob{Size: uint32(len(raw)), Data: &raw[0]}
	ent := windows.DataBlob{Size: uint32(len(entropy)), Data: &entropy[0]}
	var out windows.DataBlob
	if err := windows.CryptProtectData(&in, nil, &ent, 0, nil, windows.CRYPTPROTECT_UI_FORBIDDEN, &out); err != nil {
		t.Fatal(err)
	}
	defer windows.LocalFree(windows.Handle(uintptr(unsafe.Pointer(out.Data))))
	file := filepath.Join(dir, "token.dat")
	if err := os.WriteFile(file, unsafe.Slice(out.Data, int(out.Size)), 0600); err != nil {
		t.Fatal(err)
	}
	if token, err := readModusCreatorToken(dir); err != nil || token != string(raw) {
		t.Fatalf("DPAPI round trip failed: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, "token.json"), []byte(`{"token":"old-account"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file, []byte("damaged-current-record"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := readModusCreatorToken(dir); err == nil {
		t.Fatal("fell back to another account")
	}
}
