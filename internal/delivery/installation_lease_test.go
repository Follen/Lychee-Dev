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
	if want := "installation:" + strings.ToLower(target); resource != want {
		t.Fatalf("resource = %q, want %q", resource, want)
	}
	// A case- or short-spelled input yields the SAME identity: the derivation
	// canonicalizes before hashing, so the expected resource string is built
	// from the canonical spelling, never from the raw input.
	canonicalTarget, err := filepath.EvalSymlinks(target)
	if err != nil {
		t.Fatal(err)
	}
	againScope, againResource, err := InstallationLease(strings.ToLower(target))
	if err != nil {
		t.Fatal(err)
	}
	if againScope != scope || againResource != resource {
		t.Fatalf("case-spelled input changed the identity: (%q, %q) != (%q, %q)", againScope, againResource, scope, resource)
	}
	if want := "installation:" + strings.ToLower(canonicalTarget); resource != want {
		t.Fatalf("resource = %q, want the canonical %q", resource, want)
	}
}
