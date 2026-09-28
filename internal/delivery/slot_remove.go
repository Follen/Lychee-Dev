package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

// One durable intent owns all 65 moves. The source or archive of each member
// must still match that intent on resume; no recursive deletion or overwrite.
type addonRemoval struct {
	Schema  string               `json:"schema"`
	Parent  string               `json:"parent"`
	Archive string               `json:"archive"`
	Pool    SlotPool             `json:"pool"`
	Receipt *InstallationReceipt `json:"receipt"`
}

func removeAddonAndSlots(ctx context.Context, parent, archive string) (Removal, error) {
	_, archive, err := installationDestination(archive)
	if err != nil {
		return Removal{}, err
	}
	if err = outsideDiscovery(parent, archive); err != nil {
		return Removal{}, err
	}
	intentPath, slotArchive := archive+".removal.json", archive+".slots"
	var intent addonRemoval
	b, err := readSlotFile(intentPath, 1<<20)
	if errors.Is(err, os.ErrNotExist) {
		pool, e := readSlotPool(parent)
		if errors.Is(e, os.ErrNotExist) {
			return RemoveInstallation(ctx, filepath.Join(parent, "Lychee Dev"), archive, "addon")
		}
		if e != nil {
			return Removal{}, e
		}
		pool, e = InspectSlots(ctx, parent, pool.Version)
		if e != nil {
			return Removal{}, e
		}
		for _, entry := range pool.Files {
			if entry.PendingHash != "" || entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess {
				return Removal{}, fmt.Errorf("%w: slot reservation pending", ErrConflict)
			}
		}
		main, e := InspectInstallation(ctx, filepath.Join(parent, "Lychee Dev"), "addon")
		if e != nil {
			return Removal{}, e
		}
		if main.State != "managed" || main.Receipt == nil {
			return Removal{}, ErrInstallation
		}
		for _, path := range []string{archive, slotArchive} {
			if _, e = os.Lstat(path); !errors.Is(e, os.ErrNotExist) {
				return Removal{}, ErrConflict
			}
		}
		intent = addonRemoval{Schema: "lycheedev.addon-removal.v1", Parent: parent, Archive: archive, Pool: pool, Receipt: main.Receipt}
		b, err = json.Marshal(intent)
		if err != nil {
			return Removal{}, err
		}
		if err = vault.ReplaceFile(ctx, intentPath, b); err != nil {
			return Removal{}, err
		}
	} else if err != nil {
		return Removal{}, err
	} else if json.Unmarshal(b, &intent) != nil {
		return Removal{}, ErrInstallation
	}
	if intent.Schema != "lycheedev.addon-removal.v1" || intent.Parent != parent || intent.Archive != archive || intent.Receipt == nil || intent.Pool.Schema != "lycheedev.slots.v1" || intent.Pool.State != "ready" || len(intent.Pool.Files) != bridge.SlotCount {
		return Removal{}, ErrInstallation
	}
	if err = os.MkdirAll(slotArchive, 0700); err != nil {
		return Removal{}, err
	}
	if err = plainSlotDirectory(slotArchive); err != nil {
		return Removal{}, err
	}
	// Preflight every remaining and moved member before continuing a transaction.
	for i, entry := range intent.Pool.Files {
		location, _, e := removalLocation(SlotDirectory(parent, i+1), SlotDirectory(slotArchive, i+1))
		if e != nil {
			return Removal{}, e
		}
		if e = validateRemovalSlot(location, i+1, intent.Pool.Version, entry); e != nil {
			return Removal{}, e
		}
	}
	marker, _, err := removalLocation(filepath.Join(parent, slotMarker), filepath.Join(slotArchive, slotMarker))
	if err != nil {
		return Removal{}, err
	}
	actual, err := readSlotPool(filepath.Dir(marker))
	if err != nil {
		return Removal{}, err
	}
	want, _ := json.Marshal(intent.Pool)
	got, _ := json.Marshal(actual)
	if string(want) != string(got) {
		return Removal{}, ErrConflict
	}
	mainLocation, _, err := removalLocation(filepath.Join(parent, "Lychee Dev"), archive)
	if err != nil {
		return Removal{}, err
	}
	main, err := InspectInstallation(ctx, mainLocation, "addon")
	if err != nil {
		return Removal{}, err
	}
	want, _ = json.Marshal(intent.Receipt)
	got, _ = json.Marshal(main.Receipt)
	if main.State != "managed" || string(want) != string(got) {
		return Removal{}, ErrConflict
	}
	for i := 1; i <= bridge.SlotCount; i++ {
		if err = moveRemovalMember(ctx, SlotDirectory(parent, i), SlotDirectory(slotArchive, i)); err != nil {
			return Removal{}, err
		}
	}
	if err = moveRemovalMember(ctx, filepath.Join(parent, slotMarker), filepath.Join(slotArchive, slotMarker)); err != nil {
		return Removal{}, err
	}
	if err = moveRemovalMember(ctx, filepath.Join(parent, "Lychee Dev"), archive); err != nil {
		return Removal{}, err
	}
	return Removal{State: "archived", Archive: archive, SlotArchive: slotArchive, Receipt: intent.Receipt}, nil
}

func removalLocation(source, destination string) (string, bool, error) {
	s, se := os.Lstat(source)
	d, de := os.Lstat(destination)
	if se != nil && !errors.Is(se, os.ErrNotExist) {
		return "", false, se
	}
	if de != nil && !errors.Is(de, os.ErrNotExist) {
		return "", false, de
	}
	if (s == nil) == (d == nil) {
		return "", false, ErrConflict
	}
	if s != nil {
		return source, false, nil
	}
	return destination, true, nil
}
func validateRemovalSlot(dir string, index int, version string, entry SlotFile) error {
	if entry.PendingHash != "" || entry.Nonce != "" && !entry.Consumed && entry.RetiredRuntime == "" && !entry.RetiredProcess {
		return ErrConflict
	}
	if err := plainSlotDirectory(dir); err != nil {
		return err
	}
	if err := exactSlotFileCount(dir); err != nil {
		return err
	}
	for name, want := range slotStatic(index, version) {
		got, err := readSlotFile(filepath.Join(dir, name), 16384)
		if err != nil {
			return err
		}
		if slotDigest(got) != slotDigest(want) {
			return ErrConflict
		}
	}
	got, err := readSlotFile(filepath.Join(dir, "Payload.lua"), 2<<20)
	if err != nil {
		return err
	}
	if slotDigest(got) != entry.PayloadHash {
		return ErrConflict
	}
	return nil
}
func moveRemovalMember(ctx context.Context, source, destination string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	_, moved, err := removalLocation(source, destination)
	if err != nil || moved {
		return err
	}
	return publishDirectory(source, destination)
}
