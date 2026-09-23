//go:build windows

package live

import (
	"context"
	"testing"

	"golang.org/x/sys/windows"
)

// GitHub runner TMP is the 8.3 short path C:\Users\RUNNER~1\AppData\Local\Temp,
// so every t.TempDir() fixture spells its client directory with short names.
// reportPath must compare the resolved path against the canonical base, or a
// plain unredirected installation fails with live.report_redirected_path and
// every discovery test (and every real lookup on such a machine) returns the
// wrong fault. This is the deterministic CI-exposed regression: the same
// location under both spellings must resolve the same account.
func TestResolveReportAccountAcceptsShortPathSpelling(t *testing.T) {
	client := t.TempDir()
	realm, character := "Realm", "Character"
	createReportCharacter(t, client, "Short Path Account", realm, character)

	short := shortPathName(t, client)
	if short == client {
		t.Skip("8.3 short names are disabled on this volume")
	}
	got, err := resolveReportAccount(context.Background(), short, character, realm, "")
	if err != nil {
		t.Fatalf("resolve account through 8.3 short spelling %q: %v", short, err)
	}
	if got != "Short Path Account" {
		t.Fatalf("account = %q, want %q", got, "Short Path Account")
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
