//go:build windows && amd64

package duplexhost

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"golang.org/x/sys/windows"
)

// Windows resolves this directory independently of installation/project path
// aliases and environment variables. A process instance has exactly one claim
// namespace, even when the same process exposes several windows.
func processScope(target live.ClientWindow) (string, error) {
	if target.Window.ProcessID == 0 || target.Window.ProcessStartedAt == 0 {
		return "", errors.New("live.duplex_process_identity_required")
	}
	root, err := windows.KnownFolderPath(windows.FOLDERID_LocalAppData, windows.KF_FLAG_CREATE)
	if err != nil {
		return "", err
	}
	return filepath.Join(root, "LycheeDev", "live", "process-instances", fmt.Sprintf("%d-%d", target.Window.ProcessID, target.Window.ProcessStartedAt)), nil
}

func processClaim(claim journal.WindowOwner) journal.WindowOwner {
	// Keep the journal's window resource contract while removing HWND authority.
	var pid uint32
	var created uint64
	if _, err := fmt.Sscanf(claim.Resource, "window/%d/%d/", &pid, &created); err != nil {
		return journal.WindowOwner{}
	}
	claim.Resource = fmt.Sprintf("window/%d/%d/0", pid, created)
	return claim
}

func legacyClaimError() error {
	return errors.New("live.duplex_legacy_process_claim: finish the original CON with its original CLI before reconnecting")
}

func claimMatchesTarget(target live.ClientWindow, claim journal.WindowOwner) bool {
	return claim.Resource == fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
}

func inspectConnectionOwner(ctx context.Context, target live.ClientWindow) (journal.WindowOwner, bool, error) {
	scope, err := processScope(target)
	if err != nil {
		return journal.WindowOwner{}, false, err
	}
	resource := fmt.Sprintf("window/%d/%d/0", target.Window.ProcessID, target.Window.ProcessStartedAt)
	var owner journal.WindowOwner
	var busy bool
	if _, statErr := os.Stat(scope); statErr == nil {
		owner, busy, err = journal.InspectWindowOwner(ctx, scope, resource)
	} else if !errors.Is(statErr, os.ErrNotExist) {
		err = statErr
	}
	if err != nil || busy {
		return owner, busy, err
	}
	// A legacy connection is never silently bypassed. Its original CON must be
	// closed with the original CLI before creating a connection under this host.
	resource = fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	owner, busy, err = journal.InspectWindowOwner(ctx, addonParent(target), resource)
	if err == nil && busy {
		err = legacyClaimError()
	}
	return owner, busy, err
}

func beginConnectionClaim(ctx context.Context, target live.ClientWindow, claim journal.WindowOwner, persist func() error) error {
	if !claimMatchesTarget(target, claim) {
		return errors.New("live.duplex_process_claim_identity")
	}
	scope, err := processScope(target)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(scope, 0700); err != nil {
		return err
	}
	// Retain an installation marker for older cooperating CLIs. If either step
	// is interrupted, the durable process claim blocks a new driver; no claim is
	// fabricated or discarded to clear uncertainty.
	return journal.BeginConnectionWindow(ctx, scope, processClaim(claim), func() error {
		return journal.BeginConnectionWindow(ctx, addonParent(target), claim, persist)
	})
}

type connectionDriver struct{ instance, installation *vault.Lease }

func (d *connectionDriver) Close() error {
	return errors.Join(d.installation.Close(), d.instance.Close())
}

func lockConnectionDriver(ctx context.Context, target live.ClientWindow, claim journal.WindowOwner) (*connectionDriver, error) {
	if !claimMatchesTarget(target, claim) {
		return nil, errors.New("live.duplex_process_claim_identity")
	}
	scope, err := processScope(target)
	if err != nil {
		return nil, err
	}
	instance, err := journal.LockBootstrapWindow(ctx, scope, processClaim(claim))
	if err != nil {
		return nil, err
	}
	installation, err := journal.LockBootstrapWindow(ctx, addonParent(target), claim)
	if err != nil {
		return nil, errors.Join(err, instance.Close())
	}
	return &connectionDriver{instance: instance, installation: installation}, nil
}

func verifyConnectionClaim(ctx context.Context, target live.ClientWindow, claim journal.WindowOwner) error {
	if !claimMatchesTarget(target, claim) {
		return errors.New("live.duplex_process_claim_identity")
	}
	owner, busy, err := inspectConnectionOwner(ctx, target)
	if err != nil {
		return err
	}
	if !busy || owner != processClaim(claim) {
		return errors.New("live.duplex_process_ownership_changed")
	}
	owner, busy, err = journal.InspectWindowOwner(ctx, addonParent(target), claim.Resource)
	if err != nil {
		return err
	}
	if !busy || owner != claim {
		return errors.New("live.duplex_ownership_changed")
	}
	return nil
}

func retireConnectionClaim(ctx context.Context, target live.ClientWindow, claim journal.WindowOwner) error {
	if !claimMatchesTarget(target, claim) {
		return errors.New("live.duplex_process_claim_identity")
	}
	scope, err := processScope(target)
	if err != nil {
		return err
	}
	owner, busy, err := journal.InspectWindowOwner(ctx, scope, processClaim(claim).Resource)
	if err != nil {
		return err
	}
	if busy && owner != processClaim(claim) {
		return errors.New("live.duplex_process_ownership_changed")
	}
	// Remove the instance marker first so a held instance driver prevents any
	// retirement. The retained installation marker blocks replacement during
	// this transaction and permits retry after a crash between the removals.
	if err := journal.RetireConnectionWindow(ctx, scope, processClaim(claim)); err != nil {
		return err
	}
	return journal.RetireConnectionWindow(ctx, addonParent(target), claim)
}
