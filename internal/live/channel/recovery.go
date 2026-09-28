package channel

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrExecutionUnknown = errors.New("live.channel_execution_unknown")

type RuntimeRecovery struct {
	From        Identity     `json:"from"`
	Transaction *Transaction `json:"transaction,omitempty"`
	Phase       string       `json:"phase"`
}

// A descriptor merely selects a candidate. Fresh binding must finish before
// treating the old runtime as gone or retrying an observation in a new attempt.
func (d *Driver) RecoverRuntime(ctx context.Context, candidate Identity) error {
	if candidate.Inventory != nil && *candidate.Inventory != 64 {
		return errors.New("live.channel_slot_inventory_incomplete")
	}
	if d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		return d.finishRecovery(ctx)
	}
	old := d.State.Identity
	if candidate.Validate() != nil || candidate.Runtime <= old.Runtime || candidate.Owner != "" || candidate.GUID != old.GUID || candidate.Build != old.Build || candidate.Product != old.Product || candidate.Release != old.Release {
		return ErrPending
	}
	d.State.Recovery = &RuntimeRecovery{From: old, Transaction: d.State.Transaction, Phase: "binding"}
	d.State.Transaction = nil
	d.State.Identity = candidate
	d.State.Bound = false
	if err := d.Save(ctx, "runtime_recovery_candidate"); err != nil {
		return err
	}
	return d.finishRecovery(ctx)
}

func (d *Driver) finishRecovery(ctx context.Context) error {
	r := d.State.Recovery
	if r == nil || r.Phase == "complete" {
		return nil
	}
	if r.Phase == "binding" {
		if err := d.Connect(ctx); err != nil {
			return err
		}
		r.Phase = "bound"
		if err := d.Save(ctx, "runtime_recovery_bound"); err != nil {
			return err
		}
	}
	if r.Transaction != nil {
		if err := d.Backend.Supersede(ctx, r.Transaction.Envelope, d.State.Identity); err != nil {
			return err
		}
	}
	op := d.State.Operation
	if op != nil && op.Stage != "complete" && op.Stage != "execution_unknown" {
		// Once a commit intent exists its file may already be published, even
		// if its own input was never attempted. A delayed prior key can consume
		// that slot. Only absence of a commit transaction proves no commit yet.
		safe := op.Stage == "prepared" || op.Stage == "commit_ready" && r.Transaction == nil
		if op.Stage == "result_verified" || op.Stage == "release_ready" {
			// Fresh runtime binding proves destruction, not acknowledgement. Keep
			// the verified old result and record this distinct cleanup evidence.
			b, err := json.Marshal(op)
			if err != nil {
				return err
			}
			if err = vault.ReplaceFile(ctx, filepath.Join(d.ResultDir, op.ID+".json"), b); err != nil {
				return err
			}
			op.Stage = "complete"
			op.CleanupMethod = "runtime_destroyed"
		} else if safe || op.Policy == "observation" {
			if op.Attempt >= 3 {
				return errors.New("live.channel_recovery_attempt_limit")
			}
			ticket, err := token()
			if err != nil {
				return err
			}
			origin := d.State.Identity
			op.Ticket = ticket
			op.Attempt++
			op.Origin = &origin
			op.Stage = "prepared"
			op.PreparedNonce = ""
			op.Challenge = ""
			op.Result = nil
			op.ReportBytes = 0
			op.ReportChecksum = 0
		} else {
			op.Stage = "execution_unknown"
		}
	}
	r.Phase = "complete"
	if err := d.Save(ctx, "runtime_recovery_reconciled"); err != nil {
		return err
	}
	if op != nil && op.Stage == "execution_unknown" {
		return ErrExecutionUnknown
	}
	return nil
}
