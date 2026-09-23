//go:build !windows

package live

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// A symlinked spelling of the client directory itself is one location, not a
// redirection: reportPath compares the resolved path against the canonical
// base, so discovery through a linked client root still works. Windows has no
// equivalent here — junctions are deliberately never traversed and NTFS
// symlinks need elevated tokens — so the canonical-comparison regression on
// Windows is the 8.3 short-name spelling in
// report_account_windows_test.go.
func TestResolveReportAccountAcceptsAliasedClientSpelling(t *testing.T) {
	client := t.TempDir()
	realm, character := "Realm", "Character"
	createReportCharacter(t, client, "Alias Account", realm, character)

	alias := filepath.Join(t.TempDir(), "client-alias")
	if err := os.Symlink(client, alias); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.Remove(alias); err != nil {
			t.Error(err)
		}
	})

	got, err := resolveReportAccount(context.Background(), alias, character, realm, "")
	if err != nil {
		t.Fatalf("resolve account through aliased client spelling: %v", err)
	}
	if got != "Alias Account" {
		t.Fatalf("account = %q, want %q", got, "Alias Account")
	}
}
