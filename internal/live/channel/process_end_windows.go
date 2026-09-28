//go:build windows && amd64

package channel

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// No memory access or input: OS process lifetime is the authority for resource
// retirement only. The append-only intent retains all interrupted transactions.
func (p *Project) disconnectEnded(ctx context.Context, id string, meta projectTarget) (ProjectResult, error) {
	window := meta.Target.Window
	if meta.Owner.Resource != fmt.Sprintf("window/%d/%d/%d", window.ProcessID, window.ProcessStartedAt, window.Handle) {
		return ProjectResult{}, desktop.ErrIdentityChanged
	}
	parent := filepath.Join(meta.Target.Client.Directory, "Interface", "AddOns")
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		return ProjectResult{}, err
	}
	defer lease.Close()
	d, err := Load(p.log(id), nil)
	if err != nil {
		return ProjectResult{}, err
	}
	reason, err := desktop.ProcessEnded(ctx, meta.Target.Window)
	if err != nil {
		return present(d), err
	}
	if reason == "" {
		return present(d), desktop.ErrIdentityChanged
	}
	d.State.ProcessEnd = reason
	if err = d.Save(ctx, "process_end_observed"); err != nil {
		return present(d), err
	}
	// Publication may have died before or after its atomic payload replacement.
	// Retire only envelopes preserved in this connection, under the shared lock.
	if d.State.Transaction != nil || d.State.Recovery != nil && d.State.Recovery.Transaction != nil {
		publication, e := vault.TryAcquireLease(ctx, filepath.Join(parent, ".lycheedev-slot-locks"), "publication")
		if e != nil {
			return present(d), e
		}
		consumer := fmt.Sprintf("%d/%d", meta.Target.Window.ProcessID, meta.Target.Window.ProcessStartedAt)
		for _, tx := range []*Transaction{d.State.Transaction, recoveryTransaction(d.State.Recovery)} {
			if tx != nil {
				e = delivery.RetireSlotProcess(ctx, parent, d.State.Identity.Release, consumer, tx.Envelope)
				if e != nil {
					break
				}
			}
		}
		closeErr := publication.Close()
		if e != nil {
			return present(d), e
		}
		if closeErr != nil {
			return present(d), closeErr
		}
	}
	closeEndedState(&d.State)
	if err = d.Save(ctx, "process_end_closed"); err != nil {
		return present(d), err
	}
	if err = lease.Close(); err != nil {
		return present(d), err
	}
	return p.retire(ctx, id, present(d))
}

func recoveryTransaction(r *RuntimeRecovery) *Transaction {
	if r != nil {
		return r.Transaction
	}
	return nil
}

func closeEndedState(s *State) {
	if op := s.Operation; op != nil && op.Stage != "complete" {
		if op.Stage == "result_verified" || op.Stage == "release_ready" {
			op.Stage = "complete"
			op.CleanupMethod = "runtime_destroyed"
		} else {
			op.Stage = "execution_unknown"
			op.CleanupMethod = ""
		}
	}
	s.Bound, s.Closed, s.Closing = false, true, false
	s.Transaction, s.Reload, s.Recovery = nil, nil, nil
}
