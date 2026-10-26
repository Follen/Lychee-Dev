package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/bridge"
)

// RetireSlotProcess requires an OS-proven ended PID/creation-time consumer and
// the publication lease. It reconciles a torn publication, never a consumed ACK.
func RetireSlotProcess(ctx context.Context, parent, version, consumer string, e bridge.SlotEnvelope) error {
	if consumer == "" || e.Index < 1 || e.Index > bridge.SlotCount || e.Nonce == "" {
		return ErrInstallation
	}
	pool, available, err := slotPoolForRetirement(ctx, parent, version)
	if err != nil || !available {
		return err
	}
	if e.Index > len(pool.Files) {
		return ErrInstallation
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
	b, err := retirementPayload(parent, e.Index)
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
	if consumer == "" || e.Nonce == "" || currentRuntime == e.Runtime || len(currentRuntime) != 32 || e.Index < 1 || e.Index > bridge.SlotCount {
		return ErrInstallation
	}
	pool, available, err := slotPoolForRetirement(ctx, parent, version)
	if err != nil || !available {
		return err
	}
	if e.Index > len(pool.Files) {
		return ErrInstallation
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
	b, err := retirementPayload(parent, e.Index)
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

// The caller's process/runtime proof retires its journaled transaction even
// when the disk reservation record has been lost. Missing or malformed metadata
// cannot authorize publication and is left untouched; operational read errors
// remain errors. A readable current manifest still requires exact reservation
// reconciliation, independently of unrelated static files in the pool.
func slotPoolForRetirement(ctx context.Context, parent, version string) (SlotPool, bool, error) {
	if err := ctx.Err(); err != nil {
		return SlotPool{}, false, err
	}
	pool, err := readSlotPool(parent)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			// Windows reports a non-directory parent as not-exist too. Do not
			// turn that access failure into missing reservation metadata.
			info, parentErr := os.Stat(parent)
			if parentErr != nil && !errors.Is(parentErr, os.ErrNotExist) {
				return SlotPool{}, false, parentErr
			}
			if parentErr == nil && !info.IsDir() {
				return SlotPool{}, false, err
			}
			return SlotPool{}, false, nil
		}
		var syntax *json.SyntaxError
		var typed *json.UnmarshalTypeError
		if errors.Is(err, ErrInstallation) || errors.As(err, &syntax) || errors.As(err, &typed) {
			return SlotPool{}, false, nil
		}
		return SlotPool{}, false, err
	}
	if pool.Schema != slotPoolSchema || len(pool.Files) != bridge.SlotCount || pool.State != "ready" || pool.Version != version {
		return pool, false, ErrInstallation
	}
	return pool, true, nil
}

func retirementPayload(parent string, index int) ([]byte, error) {
	dir := SlotDirectory(parent, index)
	if err := plainSlotDirectory(dir); err != nil {
		return nil, err
	}
	return readSlotFile(filepath.Join(dir, "Payload.lua"), 2<<20)
}
