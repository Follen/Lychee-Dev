package delivery

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
)

const legacySlotMigrationSchema = "lycheedev.legacy-slots-migration.v1"

type legacySlotMigration struct {
	Schema       string   `json:"schema"`
	Parent       string   `json:"parent"`
	Archive      string   `json:"archive"`
	Version      string   `json:"version"`
	MarkerSHA256 string   `json:"markerSHA256"`
	Pool         SlotPool `json:"pool"`
}

// preflightLegacySlots admits only an exact, idle, managed legacy pool. Unknown
// directories, changed bytes and outstanding reservations block an upgrade.
func preflightLegacySlots(ctx context.Context, parent, version, archive string) (bool, error) {
	if _, err := os.Lstat(archive); !errors.Is(err, os.ErrNotExist) {
		if err == nil {
			return false, fmt.Errorf("%w: legacy slot archive already exists", ErrConflict)
		}
		return false, err
	}
	marker := filepath.Join(parent, slotMarker)
	pool, err := readExactLegacySlotPool(parent)
	if errors.Is(err, os.ErrNotExist) {
		if err = rejectUnmanagedLegacySlots(parent); err != nil {
			return false, err
		}
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("%w: legacy slot receipt is unreadable", ErrConflict)
	}
	if !managedLegacySlotPool(pool, version) {
		return false, fmt.Errorf("%w: legacy slot receipt does not match the installed addon", ErrConflict)
	}
	if _, err = inspectSlotPool(ctx, parent, pool, version); err != nil {
		return false, fmt.Errorf("%w: legacy slot files failed receipt/hash verification: %v", ErrConflict, err)
	}
	if hasPendingSlotReservation(pool) {
		return false, fmt.Errorf("%w: legacy slot reservation is pending", ErrConflict)
	}
	if _, err = os.Lstat(marker); err != nil {
		return false, err
	}
	return true, nil
}

// rejectLegacySlotsForInstall prevents an install/no-op from retaining a live
// legacy input pool. Users must use UpgradeAddon, which verifies and archives
// an idle managed pool before retiring it.
func rejectLegacySlotsForInstall(ctx context.Context, parent, version string) error {
	pool, err := readExactLegacySlotPool(parent)
	if errors.Is(err, os.ErrNotExist) {
		return rejectUnmanagedLegacySlots(parent)
	}
	if err != nil || !managedLegacySlotPool(pool, version) {
		return fmt.Errorf("%w: legacy slot receipt is unreadable or does not match the installed addon", ErrConflict)
	}
	if _, err = inspectSlotPool(ctx, parent, pool, version); err != nil {
		return fmt.Errorf("%w: legacy slot files failed receipt/hash verification: %v", ErrConflict, err)
	}
	return fmt.Errorf("%w: managed legacy slots require addon upgrade", ErrConflict)
}

// rejectUnmanagedLegacySlots intentionally does not infer ownership from a
// directory name. Without the receipt marker, every matching directory is a
// blocker and remains untouched.
func rejectUnmanagedLegacySlots(parent string) error {
	entries, err := os.ReadDir(parent)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "Lychee Dev Slot ") {
			return fmt.Errorf("%w: unmanaged legacy slot directory %s", ErrConflict, entry.Name())
		}
	}
	return nil
}

func managedLegacySlotPool(pool SlotPool, version string) bool {
	if pool.Version != version || pool.State != "ready" || pool.SourceSchema != "" {
		return false
	}
	return pool.Schema == legacySlotPoolSchema || pool.Schema == previousSlotPoolSchema || pool.Schema == slotPoolSchema
}

func hasPendingSlotReservation(pool SlotPool) bool {
	for _, slot := range pool.Files {
		if slot.PendingHash != "" || slot.Nonce != "" && !slot.Consumed && slot.RetiredRuntime == "" && !slot.RetiredProcess {
			return true
		}
	}
	return false
}

// archiveLegacySlots moves only byte-for-byte verified, idle slot pools into a
// migration archive next to the upgrade transaction. Its intent makes each
// directory rename resumable; archived payloads are never used as live input.
func archiveLegacySlots(ctx context.Context, parent, archive, version string) error {
	intentPath := archive + ".migration.json"
	intent := legacySlotMigration{Schema: legacySlotMigrationSchema, Parent: parent, Archive: archive, Version: version}
	intentBytes, err := readSlotFile(intentPath, 1<<20)
	if errors.Is(err, os.ErrNotExist) {
		pool, readErr := readExactLegacySlotPool(parent)
		if errors.Is(readErr, os.ErrNotExist) {
			return rejectUnmanagedLegacySlots(parent)
		}
		if readErr != nil || !managedLegacySlotPool(pool, version) {
			return fmt.Errorf("%w: legacy slot receipt unavailable", ErrConflict)
		}
		if _, err = inspectSlotPool(ctx, parent, pool, version); err != nil {
			return fmt.Errorf("%w: legacy slot files failed receipt/hash verification: %v", ErrConflict, err)
		}
		if hasPendingSlotReservation(pool) {
			return fmt.Errorf("%w: legacy slot reservation is pending", ErrConflict)
		}
		markerBytes, err := readSlotFile(filepath.Join(parent, slotMarker), slotPoolManifestLimit)
		if err != nil {
			return err
		}
		markerHash := sha256.Sum256(markerBytes)
		intent.MarkerSHA256 = hex.EncodeToString(markerHash[:])
		intent.Pool = pool
		if _, err = os.Lstat(archive); !errors.Is(err, os.ErrNotExist) {
			if err == nil {
				return fmt.Errorf("%w: legacy slot archive already exists", ErrConflict)
			}
			return err
		}
		intentBytes, err = json.Marshal(intent)
		if err != nil {
			return err
		}
		file, err := os.OpenFile(intentPath, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		_, writeErr := file.Write(intentBytes)
		syncErr := file.Sync()
		closeErr := file.Close()
		if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
			return err
		}
	} else if err != nil {
		return err
	} else {
		decoder := json.NewDecoder(bytes.NewReader(intentBytes))
		if err = uniqueJSON(decoder, 0); err == nil {
			decoder = json.NewDecoder(bytes.NewReader(intentBytes))
			decoder.DisallowUnknownFields()
			err = decoder.Decode(&intent)
			if err == nil {
				if _, extraErr := decoder.Token(); !errors.Is(extraErr, io.EOF) {
					err = ErrInstallation
				}
			}
		}
		if err != nil || intent.Schema != legacySlotMigrationSchema || intent.Parent != parent || intent.Archive != archive || intent.Version != version || !managedLegacySlotPool(intent.Pool, version) || hasPendingSlotReservation(intent.Pool) {
			return fmt.Errorf("%w: legacy slot migration intent is invalid", ErrConflict)
		}
	}

	if err = os.Mkdir(archive, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return err
	}
	if err = plainSlotDirectory(archive); err != nil {
		return err
	}
	if err = exactLegacySlotArchiveEntries(archive, len(intent.Pool.Files)); err != nil {
		return err
	}
	for i, entry := range intent.Pool.Files {
		source, destination := SlotDirectory(parent, i+1), SlotDirectory(archive, i+1)
		location, _, e := removalLocation(source, destination)
		if e != nil {
			return e
		}
		if e = validateRemovalSlot(location, i+1, version, intent.Pool.Schema, entry); e != nil {
			return e
		}
	}
	markerSource, markerDestination := filepath.Join(parent, slotMarker), filepath.Join(archive, slotMarker)
	marker, _, err := removalLocation(markerSource, markerDestination)
	if err != nil {
		return err
	}
	actual, err := readSlotFile(marker, slotPoolManifestLimit)
	if err != nil {
		return err
	}
	markerHash := sha256.Sum256(actual)
	if hex.EncodeToString(markerHash[:]) != intent.MarkerSHA256 {
		return fmt.Errorf("%w: legacy slot marker changed", ErrConflict)
	}
	actualPool, err := readExactLegacySlotPool(filepath.Dir(marker))
	if err != nil {
		return err
	}
	wantPool, _ := json.Marshal(intent.Pool)
	gotPool, _ := json.Marshal(actualPool)
	if string(wantPool) != string(gotPool) {
		return fmt.Errorf("%w: legacy slot receipt changed", ErrConflict)
	}

	for i := range intent.Pool.Files {
		if err = moveRemovalMember(ctx, SlotDirectory(parent, i+1), SlotDirectory(archive, i+1)); err != nil {
			return err
		}
	}
	if err = moveRemovalMember(ctx, markerSource, markerDestination); err != nil {
		return err
	}
	return os.Remove(intentPath)
}

func exactLegacySlotArchiveEntries(archive string, count int) error {
	entries, err := os.ReadDir(archive)
	if err != nil {
		return err
	}
	allowed := map[string]bool{slotMarker: true}
	for i := 1; i <= count; i++ {
		allowed[filepath.Base(SlotDirectory("", i))] = true
	}
	for _, entry := range entries {
		if !allowed[entry.Name()] {
			return fmt.Errorf("%w: unexpected file in legacy slot archive", ErrConflict)
		}
	}
	return nil
}

func readExactLegacySlotPool(parent string) (SlotPool, error) {
	var pool SlotPool
	raw, err := readSlotFile(filepath.Join(parent, slotMarker), slotPoolManifestLimit)
	if err != nil {
		return pool, err
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err = uniqueJSON(decoder, 0); err != nil {
		return pool, err
	}
	decoder = json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&pool); err != nil {
		return SlotPool{}, err
	}
	if _, err = decoder.Token(); !errors.Is(err, io.EOF) {
		return SlotPool{}, ErrInstallation
	}
	if !validSlotPoolShape(pool) {
		return SlotPool{}, ErrInstallation
	}
	return pool, nil
}
