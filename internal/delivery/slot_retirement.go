package delivery

import (
	"context"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// RetireSlotProcess requires an OS-proven ended PID/creation-time consumer and
// the publication lease. It reconciles a torn publication, never a consumed ACK.
func RetireSlotProcess(ctx context.Context, parent, version, consumer string, e bridge.SlotEnvelope) error {
	if consumer == "" || e.Index < 1 || e.Index > 64 || e.Nonce == "" {
		return ErrInstallation
	}
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	slot := &pool.Files[e.Index-1]
	if slot.Nonce != e.Nonce {
		return nil
	}
	if slot.Consumer != consumer || slot.Runtime != e.Runtime {
		return errors.New("delivery.slot_retirement_conflict")
	}
	if slot.Consumed || slot.RetiredProcess {
		return nil
	}
	b, err := readSlotFile(filepath.Join(SlotDirectory(parent, e.Index), "Payload.lua"), 2<<20)
	if err != nil {
		return err
	}
	hash := slotDigest(b)
	if hash != slot.PayloadHash && hash != slot.PendingHash {
		return ErrInstallation
	}
	slot.PayloadHash, slot.PendingHash, slot.RetiredProcess = hash, "", true
	return saveSlotPool(ctx, parent, pool)
}

// RetireSlotRuntime is called under publication admission only after a fresh
// bind proves a different runtime in the same process. It never fabricates a
// consumed receipt. Reservations of other processes/transactions are untouched.
func RetireSlotRuntime(ctx context.Context, parent, version, consumer string, e bridge.SlotEnvelope, currentRuntime string) error {
	if currentRuntime == e.Runtime || len(currentRuntime) != 32 || e.Index < 1 || e.Index > 64 {
		return ErrInstallation
	}
	pool, err := InspectSlots(ctx, parent, version)
	if err != nil {
		return err
	}
	slot := &pool.Files[e.Index-1]
	if slot.Nonce != e.Nonce {
		return nil
	}
	if slot.Consumer != consumer || slot.Runtime != e.Runtime || slot.PendingHash != "" {
		return errors.New("delivery.slot_retirement_conflict")
	}
	if slot.Consumed {
		return nil
	}
	if slot.RetiredRuntime != "" && slot.RetiredRuntime != currentRuntime {
		return errors.New("delivery.slot_retirement_conflict")
	}
	slot.RetiredRuntime = currentRuntime
	return saveSlotPool(ctx, parent, pool)
}
