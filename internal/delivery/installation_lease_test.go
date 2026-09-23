package delivery

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// Installation, upgrade, removal and queue publication must all contend for
// one OS lock per installation. These tests pin the identity derivation; the
// spelling-sensitivity cases live in the unix and windows build-tagged files
// (symlinked parents; 8.3 short names on runner TMP).
func TestInstallationLeaseIdentityShape(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "Interface", "AddOns", "Lychee Dev")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	scope, resource, err := InstallationLease(target)
	if err != nil {
		t.Fatal(err)
	}
	// The lease scope hangs off the CANONICAL parent: InstallationLease
	// resolves 8.3 short paths and symlinks before deriving the identity, so
	// the comparison must canonicalize the expectation the same way.
	canonicalParent, err := filepath.EvalSymlinks(filepath.Dir(target))
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Base(scope) != ".lycheedev-locks" || filepath.Dir(scope) != canonicalParent {
		t.Fatalf("scope = %q, want the parent-scoped lock directory of %q", scope, target)
	}
	// The derivation canonicalizes before hashing (8.3 short runner TMP,
	// symlinks), so every expectation is built from the canonical spelling.
	canonicalTarget, err := filepath.EvalSymlinks(target)
	if err != nil {
		t.Fatal(err)
	}
	if want := "installation:" + strings.ToLower(canonicalTarget); resource != want {
		t.Fatalf("resource = %q, want the canonical %q", resource, want)
	}
	// Case-spelled inputs hold the same identity only where the filesystem
	// folds case; that assertion lives in the windows build-tagged file. On
	// case-sensitive platforms a folded spelling is a different directory and
	// legitimately derives a different identity.
}
