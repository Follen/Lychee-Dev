package journal

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

// WindowRun holds the whole-operation OS lease, not a database transaction or
// input permission. The driver must Check before effects and Close when done.
// Like a window capture stream, this handle must not be used concurrently.
type WindowRun struct {
	book  *Book
	owner WindowOwner
	scope string
	lease *vault.Lease
}

func (r *WindowRun) Close() error {
	if r == nil || r.lease == nil {
		return nil
	}
	lease := r.lease
	r.lease = nil
	return lease.Close()
}

func (r *WindowRun) Check(ctx context.Context) (WorkRecord, error) {
	var zero WorkRecord
	if r == nil || r.lease == nil {
		return zero, errors.New("journal.window_run_closed")
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if err := ordinaryOwnerPath(r.scope, true); err != nil {
		return zero, err
	}
	marker := filepath.Join(r.scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(r.owner.Resource))))
	owner, err := readWindowOwner(marker, r.owner.Resource)
	if err != nil {
		return zero, err
	}
	if owner != r.owner {
		return zero, &WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != r.owner.WorkspaceID}
	}
	record, err := r.book.InspectWork(ctx, owner.OperationID)
	if err != nil {
		return zero, err
	}
	if record.Intent.Resource != owner.Resource {
		return zero, errors.New("journal.window_resource_mismatch")
	}
	if record.Stage == "cleaned" || record.Status == "completed" || record.Status == "cancelled" {
		return zero, ErrTransition
	}
	return record, nil
}

// AcquireWindowWork refuses an active driver immediately. An OS-released lease
// after process exit permits only the recorded operation to be reconciled; the
// marker is retained and this function neither replays input nor advances work.
func (b *Book) AcquireWindowWork(ctx context.Context, addonParent, workspaceID, operationID string) (run *WindowRun, err error) {
	record, err := b.InspectWork(ctx, operationID)
	if err != nil {
		return nil, err
	}
	scope, admission, err := lockWindowScope(ctx, addonParent)
	if err != nil {
		return nil, err
	}
	defer func() {
		if closeErr := admission.Close(); closeErr != nil {
			err = errors.Join(err, closeErr)
			if run != nil {
				err = errors.Join(err, run.Close())
				run = nil
			}
		}
	}()
	marker := filepath.Join(scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	owner, err := readWindowOwner(marker, record.Intent.Resource)
	if err != nil {
		return nil, err
	}
	if owner.WorkspaceID != workspaceID || owner.OperationID != operationID {
		return nil, &WindowOccupied{Owner: owner, Foreign: owner.WorkspaceID != workspaceID}
	}
	lease, err := tryWindowExecution(ctx, scope, owner.Resource)
	if err != nil {
		return nil, err
	}
	run = &WindowRun{book: b, owner: owner, scope: scope, lease: lease}
	if _, err := run.Check(ctx); err != nil {
		return nil, errors.Join(err, run.Close())
	}
	return run, nil
}

func tryWindowExecution(ctx context.Context, scope, resource string) (*vault.Lease, error) {
	key := "execution:" + resource
	path := filepath.Join(scope, fmt.Sprintf("%x.lock", sha256.Sum256([]byte(key))))
	if err := ordinaryOwnerPath(path, false); err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	lease, err := vault.TryAcquireLease(ctx, scope, key)
	if errors.Is(err, vault.ErrLeaseBusy) {
		return nil, fmt.Errorf("%w: active window driver", ErrBusy)
	}
	return lease, err
}
