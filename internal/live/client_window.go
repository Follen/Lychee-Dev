package live

import (
	"context"
	"errors"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
)

type ClientWindow struct {
	Client selection.ClientInstallation `json:"client"`
	Window desktop.WindowIdentity       `json:"window"`
}

func gameExecutable(path string) bool {
	switch strings.ToLower(filepath.Base(path)) {
	case "wow.exe", "wowclassic.exe", "wowb.exe":
		return true
	}
	return false
}

func windowExecutable(window desktop.WindowIdentity) (string, error) {
	if window.Handle == 0 || window.ProcessID == 0 || window.ProcessStartedAt == 0 || window.Class == "" || !filepath.IsAbs(window.Executable) {
		return "", errors.New("live.invalid_window_identity")
	}
	if !gameExecutable(window.Executable) {
		return "", errors.New("live.window_executable_mismatch")
	}
	return filepath.EvalSymlinks(window.Executable)
}

// Native enumeration is supplied at the I/O seam. All filter combinations use
// this one selection path and the same real installation identity reader.
func selectDiscoveredClientWindow(ctx context.Context, directory string, pid uint32, candidates []desktop.WindowIdentity) (ClientWindow, error) {
	if err := ctx.Err(); err != nil {
		return ClientWindow{}, err
	}
	var selected selection.ClientInstallation
	if directory != "" {
		var err error
		selected, err = records.InspectClientInstallation(ctx, directory)
		if err != nil {
			return ClientWindow{}, err
		}
	}
	var result ClientWindow
	for _, window := range candidates {
		if err := ctx.Err(); err != nil {
			return ClientWindow{}, err
		}
		if pid != 0 && window.ProcessID != pid {
			continue
		}
		if !gameExecutable(window.Executable) {
			if pid != 0 {
				return ClientWindow{}, errors.New("live.window_executable_mismatch")
			}
			continue
		}
		executable, err := windowExecutable(window)
		if err != nil {
			return ClientWindow{}, err
		}
		client := selected
		if directory != "" {
			if !strings.EqualFold(filepath.Dir(executable), client.Directory) {
				if pid != 0 {
					return ClientWindow{}, errors.New("live.window_installation_mismatch")
				}
				continue
			}
		} else {
			client, err = records.InspectClientInstallation(ctx, filepath.Dir(executable))
			if err != nil {
				return ClientWindow{}, err
			}
		}
		if result.Window.Handle != 0 {
			return ClientWindow{}, errors.New("live.window_ambiguous")
		}
		result = ClientWindow{Client: client, Window: window}
	}
	if result.Window.Handle == 0 {
		return ClientWindow{}, errors.New("live.window_not_found")
	}
	return result, nil
}

// ResolveClientWindow accepts optional installation and PID filters. Discovery
// is bounded to the current visible windows and never uses a title as identity.
func ResolveClientWindow(ctx context.Context, directory string, pid uint32) (ClientWindow, error) {
	windows, err := desktop.ListWindows(ctx)
	if err != nil {
		return ClientWindow{}, err
	}
	result, err := selectDiscoveredClientWindow(ctx, directory, pid, windows)
	if err != nil {
		return ClientWindow{}, err
	}
	if err := ConfirmClientWindow(ctx, result); err != nil {
		return ClientWindow{}, err
	}
	return result, nil
}

func ConfirmClientWindow(ctx context.Context, expected ClientWindow) error {
	if err := desktop.ConfirmWindow(ctx, expected.Window); err != nil {
		return err
	}
	client, err := records.InspectClientInstallation(ctx, expected.Client.Directory)
	if err != nil {
		return err
	}
	if client.Directory != expected.Client.Directory || client.Product != expected.Client.Product || client.FullBuild != expected.Client.FullBuild || client.Interface != expected.Client.Interface {
		return errors.New("live.window_client_changed")
	}
	executable, err := windowExecutable(expected.Window)
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Dir(executable), client.Directory) {
		return errors.New("live.window_installation_mismatch")
	}
	return desktop.ConfirmWindow(ctx, expected.Window)
}
