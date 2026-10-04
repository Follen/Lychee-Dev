package live

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/testkit"
)

func TestDiscoveryIsReadOnlyInstallationAndWindowInventory(t *testing.T) {
	client := testkit.Client(t, "flavor")
	exe := filepath.Join(client, "Wow.exe")
	if err := os.WriteFile(exe, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	windows := []desktop.WindowIdentity{
		{Handle: 1, ProcessID: 10, ProcessStartedAt: 20, Executable: exe, Class: "WoW", Title: "character name is not inferred"},
		{Handle: 2, ProcessID: 11, Executable: filepath.Join(client, "Other.exe")},
	}
	report, err := discoverCandidatesFrom(context.Background(), DiscoveryRequest{Roots: []string{filepath.Dir(client)}}, windows)
	if err != nil {
		t.Fatal(err)
	}
	if len(report.Installations) != 1 || report.Installations[0].State != InstallationRunning {
		t.Fatalf("installations: %+v", report.Installations)
	}
	if len(report.Windows) != 2 || report.Windows[0].Client == nil || report.Windows[1].Client != nil {
		t.Fatalf("windows: %+v", report.Windows)
	}
	if report.Windows[0].Window.Title != windows[0].Title {
		t.Fatal("window metadata lost")
	}
}

func TestDiscoveryKeepsUnsupportedClientPlatformInventory(t *testing.T) {
	report, err := discoverCandidatesFrom(context.Background(), DiscoveryRequest{}, nil)
	if err != nil || len(report.Installations) != 0 || len(report.Windows) != 0 {
		t.Fatalf("%+v %v", report, err)
	}
}
