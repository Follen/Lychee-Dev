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

// RetireSlotRuntime requires the publication lease and caller-verified evidence:
// either a fresh bind or a durably recorded proof of a different live runtime
// in the same PID/creation-time consumer under the single-current-Lua-runtime
// contract. Merely discovering a descriptor is insufficient. Runtime tokens
// are compared for equality, not chronological ordering. This reconciles torn
// publication without fabricating a consumed receipt or touching other claims.
func RetireSlotRuntime(ctx context.Context, parent, version, consumer string, e bridge.SlotEnvelope, currentRuntime string) error {
	if consumer == "" || e.Nonce == "" || currentRuntime == e.Runtime || len(currentRuntime) != 32 || e.Index < 1 || e.Index > 64 {
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
	if slot.Consumed {
		return nil
	}
	if slot.RetiredRuntime != "" && slot.RetiredRuntime != currentRuntime {
		return errors.New("delivery.slot_retirement_conflict")
	}
	b, err := readSlotFile(filepath.Join(SlotDirectory(parent, e.Index), "Payload.lua"), 2<<20)
	if err != nil {
		return err
	}
	hash := slotDigest(b)
	if hash != slot.PayloadHash && hash != slot.PendingHash {
		return ErrInstallation
	}
	slot.PayloadHash, slot.PendingHash, slot.RetiredRuntime = hash, "", currentRuntime
	return saveSlotPool(ctx, parent, pool)
}
