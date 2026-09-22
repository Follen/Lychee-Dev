package journal

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

// WindowOwner is coordination metadata only. It remains authoritative after
// process exit; absence of a live OS lock is not permission to start new work.
type WindowOwner struct {
	Schema      string `json:"schema"`
	WorkspaceID string `json:"workspaceId"`
	Resource    string `json:"resource"`
	OperationID string `json:"operationId"`
}

type WindowOccupied struct {
	Owner   WindowOwner
	Foreign bool
}

func (e *WindowOccupied) Error() string {
	if e.Foreign {
		return "journal.foreign_owner: workspace=" + e.Owner.WorkspaceID + " operation=" + e.Owner.OperationID
	}
	return "journal.window_owned: " + e.Owner.OperationID
}
func (e *WindowOccupied) Unwrap() error { return ErrBusy }

// BeginWindowWork shares admission across workspaces using the canonical addon
// parent. Only its short metadata transaction holds the OS lock. This does not
// grant input eligibility or replace the sender's whole-operation lease.
// A failed local commit leaves a recovery marker, never an unclaimed window.
// This coordinates cooperating processes; it does not promise hostile-filesystem
// isolation or survival of every filesystem/power-loss failure.
func (b *Book) BeginWindowWork(ctx context.Context, addonParent, workspaceID string, intent WorkIntent) (record WorkRecord, err error) {
	if err := ctx.Err(); err != nil {
		return record, err
	}
	if len(workspaceID) != 32 {
		return record, errors.New("journal.invalid_workspace_identity")
	}
	if _, err := hex.DecodeString(workspaceID); err != nil {
		return record, err
	}
	if !strings.HasPrefix(intent.Resource, "window/") || len(intent.Resource) > 256 {
		return record, errors.New("journal.invalid_window_resource")
	}
	scope, lease, err := lockWindowScope(ctx, addonParent)
	if err != nil {
		return record, err
	}
	defer func() { err = errors.Join(err, lease.Close()) }()
	digest := fmt.Sprintf("%x", sha256.Sum256([]byte(intent.Resource)))
	marker := filepath.Join(scope, digest+".json")
	owner, err := readWindowOwner(marker, intent.Resource)
	if err == nil {
		return record, &WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != workspaceID}
	} else if !errors.Is(err, os.ErrNotExist) {
		return record, err
	}
	entries, err := os.ReadDir(scope)
	if err != nil {
		return record, err
	}
	count := 0
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".json") {
			count++
		}
	}
	if count >= 256 {
		return record, errors.New("journal.window_owner_limit")
	}
	return b.beginWork(ctx, intent, func(candidate WorkRecord) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		owner := WindowOwner{"lycheedev.window-owner.v1", workspaceID, intent.Resource, candidate.OperationID}
		payload, err := json.Marshal(owner)
		if err != nil {
			return err
		}
		// Exclusive create intentionally leaves even a partial marker on failure:
		// another workspace must not interpret interrupted admission as freedom.
		file, err := os.OpenFile(marker, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		_, writeErr := file.Write(payload)
		syncErr := file.Sync()
		return errors.Join(writeErr, syncErr, file.Close())
	})
}

// RetireWindowWork releases only this workspace's proven cleaned operation.
// It never clears unresolved/partial claims and never removes work or evidence.
func (b *Book) RetireWindowWork(ctx context.Context, addonParent, workspaceID, operationID string) (err error) {
	record, err := b.InspectWork(ctx, operationID)
	if err != nil {
		return err
	}
	if record.Stage != "cleaned" || record.Status != "completed" && record.Status != "cancelled" {
		return ErrTransition
	}
	scope, lease, err := lockWindowScope(ctx, addonParent)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, lease.Close()) }()
	marker := filepath.Join(scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	owner, err := readWindowOwner(marker, record.Intent.Resource)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	if owner.WorkspaceID != workspaceID || owner.OperationID != operationID {
		return &WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != workspaceID}
	}
	execution, err := tryWindowExecution(ctx, scope, owner.Resource)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, execution.Close()) }()
	return os.Remove(marker)
}

// InspectWindowOwner reports recorded window ownership without creating the
// coordination scope, taking a lease, or changing any state. A marker that
// exists but cannot be read is still reported as occupied: interrupted
// admission must never look like a free window. Absence of a marker is the only
// answer that permits new input.
func InspectWindowOwner(ctx context.Context, addonParent, resource string) (WindowOwner, bool, error) {
	if err := ctx.Err(); err != nil {
		return WindowOwner{}, false, err
	}
	parent, err := filepath.Abs(addonParent)
	if err != nil {
		return WindowOwner{}, false, err
	}
	// The admission path is keyed by the canonical parent; follow it when it
	// resolves so aliases read the same markers, without requiring existence.
	if resolved, resolveErr := filepath.EvalSymlinks(parent); resolveErr == nil {
		parent = resolved
	}
	marker := filepath.Join(parent, ".lycheedev-window-owners", fmt.Sprintf("%x.json", sha256.Sum256([]byte(resource))))
	owner, err := readWindowOwner(marker, resource)
	if errors.Is(err, os.ErrNotExist) {
		return WindowOwner{}, false, nil
	}
	if err != nil {
		return WindowOwner{Schema: "lycheedev.window-owner.v1", Resource: resource}, true, err
	}
	return owner, true, nil
}

func lockWindowScope(ctx context.Context, addonParent string) (string, *vault.Lease, error) {
	if err := ctx.Err(); err != nil {
		return "", nil, err
	}
	parent, err := filepath.Abs(addonParent)
	if err != nil {
		return "", nil, err
	}
	parent, err = filepath.EvalSymlinks(parent)
	if err != nil {
		return "", nil, err
	}
	scope := filepath.Join(parent, ".lycheedev-window-owners")
	if err := os.Mkdir(scope, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return "", nil, err
	}
	if err := ordinaryOwnerPath(scope, true); err != nil {
		return "", nil, err
	}
	const admission = "window-admission"
	lockPath := filepath.Join(scope, fmt.Sprintf("%x.lock", sha256.Sum256([]byte(admission))))
	if err := ordinaryOwnerPath(lockPath, false); err != nil && !errors.Is(err, os.ErrNotExist) {
		return "", nil, err
	}
	lease, err := vault.AcquireLease(ctx, scope, admission)
	if err != nil {
		return "", nil, err
	}
	if err := ordinaryOwnerPath(scope, true); err != nil {
		return "", nil, errors.Join(err, lease.Close())
	}
	return scope, lease, nil
}

func ordinaryOwnerPath(path string, directory bool) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if info.Mode()&os.ModeSymlink != 0 || directory && !info.IsDir() || !directory && !info.Mode().IsRegular() {
		return errors.New("journal.redirected_window_owner")
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return err
	}
	if !strings.EqualFold(filepath.Clean(path), filepath.Clean(resolved)) {
		return errors.New("journal.redirected_window_owner")
	}
	return nil
}

func readWindowOwner(path, resource string) (WindowOwner, error) {
	var owner WindowOwner
	if err := ordinaryOwnerPath(path, false); err != nil {
		return owner, err
	}
	file, err := os.Open(path)
	if err != nil {
		return owner, err
	}
	data, readErr := io.ReadAll(io.LimitReader(file, 4097))
	if err := errors.Join(readErr, file.Close()); err != nil {
		return owner, err
	}
	if len(data) > 4096 {
		return owner, errors.New("journal.invalid_window_owner")
	}
	if err := json.Unmarshal(data, &owner); err != nil {
		return owner, err
	}
	canonical, _ := json.Marshal(owner)
	if !bytes.Equal(data, canonical) || owner.Schema != "lycheedev.window-owner.v1" || owner.Resource != resource || len(owner.WorkspaceID) != 32 || !strings.HasPrefix(owner.OperationID, "OP-") || len(owner.OperationID) != 35 {
		return owner, errors.New("journal.invalid_window_owner")
	}
	if _, err := hex.DecodeString(owner.WorkspaceID + strings.TrimPrefix(owner.OperationID, "OP-")); err != nil {
		return owner, err
	}
	return owner, nil
}
