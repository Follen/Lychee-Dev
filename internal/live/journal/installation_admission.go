package journal

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

// BeginBootstrapWindow shares the same admission scope as operations and
// maintenance. The callback commits the local bootstrap body before publication.
func BeginBootstrapWindow(ctx context.Context, parent string, owner WindowOwner, persist func() error) (err error) {
	return beginScopedWindow(ctx, parent, owner, persist, "BTP-")
}

// BeginConnectionWindow publishes a durable connection claim in the SAME
// admission namespace used by existing operations and installation maintenance.
func BeginConnectionWindow(ctx context.Context, parent string, owner WindowOwner, persist func() error) error {
	return beginScopedWindow(ctx, parent, owner, persist, "CON-")
}

func beginScopedWindow(ctx context.Context, parent string, owner WindowOwner, persist func() error, prefix string) (err error) {
	if !strings.HasPrefix(owner.OperationID, prefix) || len(owner.OperationID) != 36 || len(owner.WorkspaceID) != 32 ||
		!strings.HasPrefix(owner.Resource, "window/") || len(owner.Resource) > 256 || len(owner.IntentSHA256) != 64 || persist == nil {
		return errors.New("journal.invalid_bootstrap_claim")
	}
	if _, err := hex.DecodeString(owner.OperationID[4:] + owner.WorkspaceID + owner.IntentSHA256); err != nil {
		return err
	}
	owner.Schema = "lycheedev.window-owner.v1"
	scope, gate, err := lockWindowScope(ctx, parent)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, gate.Close()) }()
	marker := filepath.Join(scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(owner.Resource))))
	current, err := readWindowOwner(marker, owner.Resource)
	if err == nil {
		return &WindowOccupied{Owner: current, Foreign: current.WorkspaceID != owner.WorkspaceID}
	}
	if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	entries, err := os.ReadDir(scope)
	if err != nil {
		return err
	}
	count := 0
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".json") {
			count++
		}
	}
	if count >= 256 {
		return errors.New("journal.window_owner_limit")
	}
	if err := persist(); err != nil {
		return err
	}
	return publishWindowOwner(ctx, scope, owner)
}

// LockBootstrapWindow prevents abandon from racing native input. The durable
// marker survives release of this short driver lease.
func LockBootstrapWindow(ctx context.Context, parent string, want WindowOwner) (lease *vault.Lease, err error) {
	scope, gate, err := lockWindowScope(ctx, parent)
	if err != nil {
		return nil, err
	}
	defer func() { err = errors.Join(err, gate.Close()) }()
	owner, busy, err := InspectWindowOwner(ctx, parent, want.Resource)
	if err != nil {
		return nil, err
	}
	if !busy || owner != want {
		return nil, ErrBusy
	}
	return tryWindowExecution(ctx, scope, want.Resource)
}

// RetireBootstrapWindow is called only after the local confirmed/abandoned
// record is durable. It cannot release an operation or a foreign attempt.
func RetireBootstrapWindow(ctx context.Context, parent string, want WindowOwner) (err error) {
	return retireScopedWindow(ctx, parent, want, "BTP-")
}

func RetireConnectionWindow(ctx context.Context, parent string, want WindowOwner) error {
	return retireScopedWindow(ctx, parent, want, "CON-")
}

func retireScopedWindow(ctx context.Context, parent string, want WindowOwner, prefix string) (err error) {
	if !strings.HasPrefix(want.OperationID, prefix) {
		return ErrTransition
	}
	scope, gate, err := lockWindowScope(ctx, parent)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, gate.Close()) }()
	owner, busy, err := InspectWindowOwner(ctx, parent, want.Resource)
	if err != nil || !busy {
		return err
	}
	if owner != want {
		return &WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != want.WorkspaceID}
	}
	lease, err := tryWindowExecution(ctx, scope, want.Resource)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, lease.Close()) }()
	return os.Remove(filepath.Join(scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(want.Resource)))))
}

// Installation maintenance never waits for an operation to finish while
// preventing that operation from making progress. Existing claims refuse it
// immediately; an empty scope stays locked only across the file mutation.
func AcquireInstallationMaintenance(ctx context.Context, parent string) (lease *vault.Lease, err error) {
	scope, gate, err := lockWindowScope(ctx, parent)
	if err != nil {
		return nil, err
	}
	entries, err := os.ReadDir(scope)
	if err == nil {
		for _, entry := range entries {
			if strings.HasSuffix(entry.Name(), ".json") {
				err = fmt.Errorf("%w: installation has unresolved window claims", ErrBusy)
				break
			}
		}
	}
	if err != nil {
		return nil, errors.Join(err, gate.Close())
	}
	return gate, nil
}
