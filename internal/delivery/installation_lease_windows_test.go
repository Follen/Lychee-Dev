//go:build windows

package delivery

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

// GitHub runner TMP is the 8.3 short path C:\Users\RUNNER~1\... while the
// canonical spelling is C:\Users\runneradmin\.... Both spellings of the same
// installation must select the same lease, or holders of one spelling never
// exclude holders of the other.
func TestInstallationLeaseShortPathSpelling(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "Interface", "AddOns", "Lychee Dev")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	wantScope, wantResource, err := InstallationLease(target)
	if err != nil {
		t.Fatal(err)
	}
	short := shortPathName(t, target)
	if short == target {
		t.Skip("8.3 short names are disabled on this volume")
	}
	gotScope, gotResource, err := InstallationLease(short)
	if err != nil {
		t.Fatal(err)
	}
	if gotScope != wantScope || gotResource != wantResource {
		t.Fatalf("short-path spelling split the lease identity:\n(%q, %q)\n!= (%q, %q)", gotScope, gotResource, wantScope, wantResource)
	}
}

func shortPathName(t *testing.T, path string) string {
	t.Helper()
	pointer, err := windows.UTF16PtrFromString(path)
	if err != nil {
		t.Fatal(err)
	}
	length, err := windows.GetShortPathName(pointer, nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if length == 0 {
		return path
	}
	buffer := make([]uint16, length)
	if _, err = windows.GetShortPathName(pointer, &buffer[0], length); err != nil {
		t.Fatal(err)
	}
	return windows.UTF16ToString(buffer)
}

// Windows filesystems fold path case, so a lower- or upper-spelled
// installation input must produce exactly one lease identity.
func TestInstallationLeaseIdentityCaseFold(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "Interface", "AddOns", "Lychee Dev")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	scope, resource, err := InstallationLease(target)
	if err != nil {
		t.Fatal(err)
	}
	foldedScope, foldedResource, err := InstallationLease(strings.ToLower(target))
	if err != nil {
		t.Fatal(err)
	}
	if foldedScope != scope || foldedResource != resource {
		t.Fatalf("case-spelled input changed the identity: (%q, %q) != (%q, %q)", foldedScope, foldedResource, scope, resource)
	}
}
