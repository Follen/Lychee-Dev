//go:build !windows

package delivery

import (
	"os"
	"path/filepath"
	"testing"
)

// A symlinked discovery parent is one installation, not two: the lease identity
// resolves the parent through filepath.EvalSymlinks, so both spellings select
// the same lock file. (Windows junctions are deliberately treated as redirected
// paths by the mutation flows and are covered by the dedicated rejection tests.)
func TestInstallationLeaseAliasedParentSpelling(t *testing.T) {
	root := t.TempDir()
	parent := filepath.Join(root, "Interface", "AddOns")
	target := filepath.Join(parent, "Lychee Dev")
	if err := os.MkdirAll(target, 0700); err != nil {
		t.Fatal(err)
	}
	wantScope, wantResource, err := InstallationLease(target)
	if err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "AddOns Alias")
	if err := os.Symlink(parent, alias); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.Remove(alias); err != nil {
			t.Error(err)
		}
	})
	gotScope, gotResource, err := InstallationLease(filepath.Join(alias, "Lychee Dev"))
	if err != nil {
		t.Fatal(err)
	}
	if gotScope != wantScope || gotResource != wantResource {
		t.Fatalf("aliased spelling split the lease identity: (%q, %q) != (%q, %q)", gotScope, gotResource, wantScope, wantResource)
	}
}
