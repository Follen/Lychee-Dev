// Historical records only: no game capture, input, or execution adapters.
package live

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"path/filepath"
)

func windowOwnership(ctx context.Context, workspaceID string, target ClientWindow, io *liveIO) (journal.WindowOwner, bool, bool, error) {
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	owner, occupied, err := io.peek(ctx, parent, windowResource(target))
	if err != nil {
		return owner, true, true, err
	}
	if !occupied {
		return owner, false, false, nil
	}
	return owner, true, owner.WorkspaceID != workspaceID, nil
}

func windowResource(target ClientWindow) string {
	return windowHandleResource(target.Window)
}

func windowHandleResource(window desktop.WindowIdentity) string {
	return fmt.Sprintf("window/%d/%d/%d", window.ProcessID, window.ProcessStartedAt, window.Handle)
}

const (
	CandidateBusy       = "busy"
	CandidateUnreadable = "identity_unreadable"
)

type liveIO struct {
	root    string
	list    func(context.Context) ([]desktop.WindowIdentity, error)
	inspect func(context.Context, string) (selection.ClientInstallation, error)
	peek    func(context.Context, string, string) (journal.WindowOwner, bool, error)
}

func nativeIO() *liveIO {
	return &liveIO{list: desktop.ListWindows, inspect: records.InspectClientInstallation, peek: journal.InspectWindowOwner}
}
