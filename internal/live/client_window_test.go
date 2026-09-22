package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"strconv"
	"testing"
)

func TestClientWindowSelection(t *testing.T) {
	directory := testkit.Client(t, "flavor")
	client, err := selection.InspectClient(context.Background(), directory, nil)
	if err != nil {
		t.Fatal(err)
	}
	executable := filepath.Join(directory, "Wow.exe")
	if err := os.WriteFile(executable, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	window := desktop.WindowIdentity{Handle: 10, ProcessID: 20, ProcessStartedAt: 30, Executable: executable, Class: "fixture", Title: "Not a character identity"}
	got, err := selectDiscoveredClientWindow(context.Background(), client.Directory, 20, []desktop.WindowIdentity{window})
	if err != nil || got.Client != client || got.Window != window {
		t.Fatalf("%+v %v", got, err)
	}
	other := window
	other.ProcessID = 21
	if _, err := selectDiscoveredClientWindow(context.Background(), client.Directory, 20, []desktop.WindowIdentity{other, window}); err != nil {
		t.Fatal(err)
	}
	for _, bad := range [][]desktop.WindowIdentity{nil, {other}, {window, window}} {
		if _, err := selectDiscoveredClientWindow(context.Background(), client.Directory, 20, bad); err == nil {
			t.Fatal("missing/ambiguous window accepted")
		}
	}
	for _, change := range []func(*desktop.WindowIdentity){func(w *desktop.WindowIdentity) { w.ProcessStartedAt = 0 }, func(w *desktop.WindowIdentity) { w.Handle = 0 }, func(w *desktop.WindowIdentity) { w.Class = "" }, func(w *desktop.WindowIdentity) { w.Executable = "Wow.exe" }, func(w *desktop.WindowIdentity) { w.Executable = filepath.Join(directory, "Other.exe") }} {
		bad := window
		change(&bad)
		if _, err := selectDiscoveredClientWindow(context.Background(), client.Directory, 20, []desktop.WindowIdentity{bad}); err == nil {
			t.Fatal("invalid identity accepted")
		}
	}
	foreign := t.TempDir()
	foreignExecutable := filepath.Join(foreign, "Wow.exe")
	if err := os.WriteFile(foreignExecutable, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	other = window
	other.Executable = foreignExecutable
	if _, err := selectDiscoveredClientWindow(context.Background(), client.Directory, 20, []desktop.WindowIdentity{other}); err == nil {
		t.Fatal("another installation accepted")
	}
}

func testGameWindow(t *testing.T, directory string, pid uint32) desktop.WindowIdentity {
	t.Helper()
	executable := filepath.Join(directory, "Wow.exe")
	if err := os.WriteFile(executable, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	return desktop.WindowIdentity{Handle: uint64(pid) + 100, ProcessID: pid, ProcessStartedAt: uint64(pid) + 200, Executable: executable, Class: "fixture", Title: "Same title is not identity"}
}

func requireWindowCode(t *testing.T, err error, code string) {
	t.Helper()
	if err == nil || err.Error() != code {
		t.Fatalf("error = %v, want %s", err, code)
	}
}

func TestDiscoveredClientWindowInfersInstallationAndOptionalPID(t *testing.T) {
	directory := testkit.Client(t, "flavor")
	window := testGameWindow(t, directory, 41)

	got, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{window})
	if err != nil {
		t.Fatal(err)
	}
	if got.Window != window || got.Client.Directory != directory {
		t.Fatalf("got %+v, want window and directory %q", got, directory)
	}

	got, err = selectDiscoveredClientWindow(context.Background(), "", window.ProcessID, []desktop.WindowIdentity{window})
	if err != nil {
		t.Fatal(err)
	}
	if got.Window != window {
		t.Fatalf("PID-filtered window = %+v, want %+v", got.Window, window)
	}

	got, err = selectDiscoveredClientWindow(context.Background(), directory, 0, []desktop.WindowIdentity{window})
	if err != nil {
		t.Fatal(err)
	}
	if got.Client.Directory != directory || got.Window != window {
		t.Fatalf("installation-filtered result = %+v", got)
	}
}

func TestDiscoveredClientWindowAmbiguityDoesNotUseTitle(t *testing.T) {
	directory := testkit.Client(t, "flavor")
	first := testGameWindow(t, directory, 51)
	second := first
	second.Handle++
	second.ProcessID++
	second.ProcessStartedAt++
	second.Title = first.Title

	_, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{first, second})
	requireWindowCode(t, err, "live.window_ambiguous")
}

func TestDiscoveredClientWindowFiltersNonWoWExecutables(t *testing.T) {
	directory := testkit.Client(t, "flavor")
	valid := testGameWindow(t, directory, 61)
	foreignExecutable := filepath.Join(t.TempDir(), "not-game.exe")
	if err := os.WriteFile(foreignExecutable, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	foreign := valid
	foreign.Handle++
	foreign.ProcessID++
	foreign.ProcessStartedAt++
	foreign.Executable = foreignExecutable

	got, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{foreign, valid})
	if err != nil {
		t.Fatal(err)
	}
	if got.Window != valid {
		t.Fatalf("got %+v, want WoW candidate %+v", got.Window, valid)
	}

	_, err = selectDiscoveredClientWindow(context.Background(), "", foreign.ProcessID, []desktop.WindowIdentity{foreign})
	requireWindowCode(t, err, "live.window_executable_mismatch")
}

func TestDiscoveredClientWindowExplicitInstallationMismatch(t *testing.T) {
	selected := testkit.Client(t, "flavor")
	other := testkit.Client(t, "flavor")
	window := testGameWindow(t, other, 71)

	_, err := selectDiscoveredClientWindow(context.Background(), selected, window.ProcessID, []desktop.WindowIdentity{window})
	requireWindowCode(t, err, "live.window_installation_mismatch")
}

func TestDiscoveredClientWindowUsesCanonicalInstallationIdentity(t *testing.T) {
	realDirectory := testkit.Client(t, "flavor")
	aliasParent := t.TempDir()
	aliasDirectory := filepath.Join(aliasParent, "client-link")
	if err := os.Symlink(realDirectory, aliasDirectory); err != nil {
		t.Skipf("directory symlinks unavailable: %v", err)
	}
	window := testGameWindow(t, realDirectory, 81)

	got, err := selectDiscoveredClientWindow(context.Background(), aliasDirectory, window.ProcessID, []desktop.WindowIdentity{window})
	if err != nil {
		t.Fatal(err)
	}
	canonical, err := filepath.EvalSymlinks(realDirectory)
	if err != nil {
		t.Fatal(err)
	}
	if got.Client.Directory != canonical {
		t.Fatalf("directory = %q, want canonical %q", got.Client.Directory, canonical)
	}
}

func TestDiscoveredClientWindowReturnsSelectedMetadataErrors(t *testing.T) {
	t.Run("unsupported", func(t *testing.T) {
		directory := testkit.Client(t, "unsupported")
		window := testGameWindow(t, directory, 91)
		_, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{window})
		if !errors.Is(err, selection.ErrClientIdentity) {
			t.Fatalf("error = %v, want client identity error", err)
		}
	})

	t.Run("broken metadata", func(t *testing.T) {
		directory := testkit.Client(t, "flavor")
		if err := os.WriteFile(filepath.Join(directory, "version.txt"), []byte("not-a-build"), 0600); err != nil {
			t.Fatal(err)
		}
		window := testGameWindow(t, directory, 92)
		_, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{window})
		if !errors.Is(err, selection.ErrClientIdentity) {
			t.Fatalf("error = %v, want client identity error", err)
		}
	})

	t.Run("invalid candidate is not skipped", func(t *testing.T) {
		directory := testkit.Client(t, "flavor")
		invalid := testGameWindow(t, directory, 93)
		invalid.Class = ""
		valid := testGameWindow(t, directory, 94)
		_, err := selectDiscoveredClientWindow(context.Background(), "", 0, []desktop.WindowIdentity{invalid, valid})
		requireWindowCode(t, err, "live.invalid_window_identity")
	})

	t.Run("missing candidate", func(t *testing.T) {
		_, err := selectDiscoveredClientWindow(context.Background(), "", 0, nil)
		requireWindowCode(t, err, "live.window_not_found")
	})
}

func TestClientWindowLiveReadOnly(t *testing.T) {
	directory := os.Getenv("LYCHEEDEV_LIVE_READONLY_CLIENT")
	if directory == "" {
		t.Skip("explicit live read-only target required")
	}
	pid, err := strconv.ParseUint(os.Getenv("LYCHEEDEV_LIVE_READONLY_PID"), 10, 32)
	if err != nil {
		t.Fatal(err)
	}
	bound, err := ResolveClientWindow(context.Background(), directory, uint32(pid))
	if err != nil {
		t.Fatal(err)
	}
	if err := ConfirmClientWindow(context.Background(), bound); err != nil {
		t.Fatal(err)
	}
	changed := bound
	changed.Window.ProcessStartedAt++
	if err := ConfirmClientWindow(context.Background(), changed); err == nil {
		t.Fatal("changed process identity accepted")
	}
	changed = bound
	changed.Client.FullBuild = "0.0.0.0"
	if err := ConfirmClientWindow(context.Background(), changed); err == nil {
		t.Fatal("changed build accepted")
	}
	t.Logf("readonly product=%s build=%s pid=%d hwnd=%d", bound.Client.Product, bound.Client.FullBuild, bound.Window.ProcessID, bound.Window.Handle)
}
