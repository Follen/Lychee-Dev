package live

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"testing"
)

func TestResolveReportAccountExplicitOverride(t *testing.T) {
	client := t.TempDir()
	want := "First Login 账号"

	got, err := resolveReportAccount(context.Background(), client, "Character", "Realm", want)
	if err != nil {
		t.Fatalf("resolve explicit account: %v", err)
	}
	if got != want {
		t.Fatalf("account = %q, want %q", got, want)
	}

	for _, account := range []string{"", ".", "..", "../Account", `Account\Child`, "Account:stream", "Account.", "Account ", "bad\x00"} {
		if _, err := resolveReportAccount(context.Background(), client, "Character", "Realm", account); err == nil {
			t.Fatalf("invalid explicit account %q was accepted", account)
		}
	}
}

func TestResolveReportAccountDiscovery(t *testing.T) {
	client := t.TempDir()
	realm := "Realm With Space 领域"
	character := "Character 名称"

	createReportCharacter(t, client, "Alpha 账号", realm, character)
	createReportCharacter(t, client, "Noise Account", "Other Realm", character)
	createReportCharacter(t, client, "Another Noise", realm, "Other Character")

	got, err := resolveReportAccount(context.Background(), client, character, realm, "")
	if err != nil {
		t.Fatalf("resolve unique account: %v", err)
	}
	if got != "Alpha 账号" {
		t.Fatalf("account = %q, want %q", got, "Alpha 账号")
	}

	createReportCharacter(t, client, "Zeta Account", realm, character)
	_, err = resolveReportAccount(context.Background(), client, character, realm, "")
	assertAccountSelection(t, err, []string{"Alpha 账号", "Zeta Account"})
}

func TestResolveReportAccountDoesNotReadSavedVariables(t *testing.T) {
	client := t.TempDir()
	realm := "Realm With Space"
	character := "Character With Space"
	characterDirectory := createReportCharacter(t, client, "Metadata Only", realm, character)

	accountDirectory := filepath.Dir(filepath.Dir(characterDirectory))
	sentinels := map[string][]byte{
		filepath.Join(accountDirectory, "SavedVariables", "Lychee Dev.lua"):     []byte("account-level corrupt Lua sentinel"),
		filepath.Join(accountDirectory, "SavedVariables", "Lychee Dev.lua.bak"): []byte("account-level backup sentinel"),
		filepath.Join(accountDirectory, "SavedVariables", "legacy.lua"):         []byte("account-level legacy sentinel"),
		filepath.Join(accountDirectory, "SavedVariables.bak"):                   []byte("account-level directory backup sentinel"),
		filepath.Join(characterDirectory, "SavedVariables", "Lychee Dev.lua"):   []byte("character-level corrupt Lua sentinel"),
	}
	for path, want := range sentinels {
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, want, 0600); err != nil {
			t.Fatal(err)
		}
	}

	got, err := resolveReportAccount(context.Background(), client, character, realm, "")
	if err != nil {
		t.Fatalf("resolve account with corrupt SavedVariables: %v", err)
	}
	if got != "Metadata Only" {
		t.Fatalf("account = %q, want %q", got, "Metadata Only")
	}
	for path, want := range sentinels {
		contents, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read sentinel %q: %v", path, err)
		}
		if !bytes.Equal(contents, want) {
			t.Fatalf("sentinel %q changed from %q to %q", path, want, contents)
		}
	}
}

func TestResolveReportAccountMissingDiscoveryRoot(t *testing.T) {
	_, err := resolveReportAccount(context.Background(), t.TempDir(), "Character", "Realm", "")
	assertAccountSelection(t, err, []string{})
}

func TestResolveReportAccountRejectsInvalidPathSegments(t *testing.T) {
	client := t.TempDir()
	cases := []struct {
		name      string
		character string
		realm     string
	}{
		{name: "empty character", character: "", realm: "Realm"},
		{name: "dot character", character: ".", realm: "Realm"},
		{name: "parent character", character: "../Character", realm: "Realm"},
		{name: "slash character", character: "Character/Child", realm: "Realm"},
		{name: "empty realm", character: "Character", realm: ""},
		{name: "dot realm", character: "Character", realm: "."},
		{name: "parent realm", character: "Character", realm: "../Realm"},
		{name: "slash realm", character: "Character", realm: `Realm\Child`},
		{name: "colon realm", character: "Character", realm: "Realm:stream"},
		{name: "nul realm", character: "Character", realm: "Realm\x00"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := resolveReportAccount(context.Background(), client, tc.character, tc.realm, ""); err == nil {
				t.Fatalf("accepted character %q and realm %q", tc.character, tc.realm)
			}
		})
	}
}

func TestResolveReportAccountCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := resolveReportAccount(ctx, t.TempDir(), "Character", "Realm", "")
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
}

func TestResolveReportAccountScanLimitIncludesNonDirectories(t *testing.T) {
	client := t.TempDir()
	accountRoot := filepath.Join(client, "WTF", "Account")
	if err := os.MkdirAll(accountRoot, 0700); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 257; i++ {
		path := filepath.Join(accountRoot, "entry-"+formatReportAccountIndex(i))
		if i == 256 {
			if err := os.WriteFile(path, []byte("noise"), 0600); err != nil {
				t.Fatal(err)
			}
			continue
		}
		if err := os.Mkdir(path, 0700); err != nil {
			t.Fatal(err)
		}
	}

	_, err := resolveReportAccount(context.Background(), client, "Character", "Realm", "")
	if !errors.Is(err, ErrAccountScanLimit) {
		t.Fatalf("error = %v, want ErrAccountScanLimit", err)
	}
}

func TestResolveReportAccountAcceptsScanBoundary(t *testing.T) {
	client := t.TempDir()
	accountRoot := filepath.Join(client, "WTF", "Account")
	if err := os.MkdirAll(accountRoot, 0700); err != nil {
		t.Fatal(err)
	}
	createReportCharacter(t, client, "Boundary Account", "Realm", "Character")
	for i := 0; i < 255; i++ {
		if err := os.Mkdir(filepath.Join(accountRoot, "noise-"+formatReportAccountIndex(i)), 0700); err != nil {
			t.Fatal(err)
		}
	}

	got, err := resolveReportAccount(context.Background(), client, "Character", "Realm", "")
	if err != nil {
		t.Fatalf("resolve at 256-entry boundary: %v", err)
	}
	if got != "Boundary Account" {
		t.Fatalf("account = %q, want %q", got, "Boundary Account")
	}
}

func TestResolveReportAccountRejectsNonDirectoryLeaf(t *testing.T) {
	client := t.TempDir()
	realm := "Realm"
	character := "Character"
	leaf := filepath.Join(client, "WTF", "Account", "Leaf Account", realm, character)
	if err := os.MkdirAll(filepath.Dir(leaf), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(leaf, []byte("not a directory"), 0600); err != nil {
		t.Fatal(err)
	}

	_, err := resolveReportAccount(context.Background(), client, character, realm, "")
	assertReportRedirectedPath(t, err)
}

func TestResolveReportAccountRejectsRedirectedPath(t *testing.T) {
	client := t.TempDir()
	accountRoot := filepath.Join(client, "WTF", "Account")
	if err := os.MkdirAll(accountRoot, 0700); err != nil {
		t.Fatal(err)
	}
	outside := t.TempDir()
	if err := os.MkdirAll(filepath.Join(outside, "Realm", "Character"), 0700); err != nil {
		t.Fatal(err)
	}
	redirected := filepath.Join(accountRoot, "Redirected Account")
	makeReportDirectoryRedirect(t, redirected, outside)

	_, err := resolveReportAccount(context.Background(), client, "Character", "Realm", "")
	assertReportRedirectedPath(t, err)
}

func createReportCharacter(t *testing.T, client, account, realm, character string) string {
	t.Helper()
	path := filepath.Join(client, "WTF", "Account", account, realm, character)
	if err := os.MkdirAll(path, 0700); err != nil {
		t.Fatal(err)
	}
	return path
}

func assertAccountSelection(t *testing.T, err error, want []string) {
	t.Helper()
	if err == nil {
		t.Fatalf("expected AccountSelectionError with candidates %v", want)
	}
	var selection *AccountSelectionError
	if !errors.As(err, &selection) {
		t.Fatalf("error = %T %v, want *AccountSelectionError", err, err)
	}
	if !reflect.DeepEqual(selection.Candidates, want) {
		t.Fatalf("candidates = %#v, want %#v", selection.Candidates, want)
	}
	if !sort.StringsAreSorted(selection.Candidates) {
		t.Fatalf("candidates are not sorted: %#v", selection.Candidates)
	}
}

func assertReportRedirectedPath(t *testing.T, err error) {
	t.Helper()
	if err == nil || err.Error() != "live.report_redirected_path" {
		t.Fatalf("error = %v, want live.report_redirected_path", err)
	}
}

func formatReportAccountIndex(index int) string {
	return fmt.Sprintf("%03d", index)
}

func makeReportDirectoryRedirect(t *testing.T, link, target string) {
	t.Helper()
	if runtime.GOOS == "windows" {
		command := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:LYCHEEDEV_TEST_LINK -Target $env:LYCHEEDEV_TEST_TARGET | Out-Null")
		command.Env = append(os.Environ(), "LYCHEEDEV_TEST_LINK="+link, "LYCHEEDEV_TEST_TARGET="+target)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("junction: %v %s", err, output)
		}
	} else if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.Remove(link); err != nil {
			t.Error(err)
		}
	})
}
