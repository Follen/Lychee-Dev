package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// SlotReset reports a completed replacement of the fixed slot pool. Archive
// retains the original bytes, including unresolved reservations and payloads.
type SlotReset struct {
	Archive  string `json:"archive"`
	Complete bool   `json:"complete"`
}

type slotResetMember struct {
	Name   string `json:"name"`
	Digest string `json:"digest,omitempty"` // Empty means absent at intent creation.
}

type slotResetIntent struct {
	Schema  string              `json:"schema"`
	Parent  string              `json:"parent"`
	Archive string              `json:"archive"`
	Version string              `json:"version"`
	Phase   string              `json:"phase"`
	Receipt InstallationReceipt `json:"receipt"`
	Members []slotResetMember   `json:"members"`
}

// ResetAddonSlots archives the original manifest and fixed 500 slot paths, then
// installs an initial pool beneath an unchanged, clean managed main addon. It
// deliberately does not interpret the old manifest or settle old business.
// Retrying one archive resumes its durable intent; a completed retry only
// verifies the archive and current pool, preserving later slot reservations.
func ResetAddonSlots(ctx context.Context, clientDirectory, archive, version string) (result SlotReset, err error) {
	_, parent, err := ResolveAddonDestination(ctx, clientDirectory)
	if err != nil {
		return result, err
	}
	if archive == "" || version == "" {
		return result, ErrInstallation
	}
	abs, err := filepath.Abs(archive)
	if err != nil {
		return result, err
	}
	// ResolveAddonDestination rejects redirected discovery directories. Apply
	// the same rule to the archive parent before deriving its canonical path.
	if err = plainSlotDirectory(filepath.Dir(abs)); err != nil {
		return result, err
	}
	_, archive, err = installationDestination(abs)
	if err != nil {
		return result, err
	}
	result.Archive = archive
	if err = outsideDiscovery(parent, archive); err != nil {
		return result, err
	}
	if !strings.EqualFold(filepath.VolumeName(parent), filepath.VolumeName(archive)) {
		return result, fmt.Errorf("%w: slot archive must be on the installation volume", ErrInstallation)
	}
	gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
	if err != nil {
		return result, err
	}
	defer func() { err = errors.Join(err, gate.Close()) }()
	mainDirectory := filepath.Join(parent, "Lychee Dev")
	if err = plainSlotDirectory(mainDirectory); err != nil {
		return result, err
	}
	main, err := InspectInstallation(ctx, mainDirectory, "addon")
	if err != nil {
		return result, err
	}
	if main.State != "managed" || main.Receipt == nil || main.Receipt.Version != version || !receiptUsesSlots(*main.Receipt) {
		return result, fmt.Errorf("%w: slot reset requires the clean managed addon at version %s", ErrConflict, version)
	}

	intentPath := archive + ".slot-reset.json"
	var intent slotResetIntent
	raw, err := readSlotFile(intentPath, 1<<20)
	if errors.Is(err, os.ErrNotExist) {
		if _, checkErr := os.Lstat(archive); !errors.Is(checkErr, os.ErrNotExist) {
			if checkErr != nil {
				return result, checkErr
			}
			return result, fmt.Errorf("%w: slot archive already exists without its reset intent", ErrConflict)
		}
		intent = slotResetIntent{Schema: "lycheedev.slot-reset.v1", Parent: parent, Archive: archive, Version: version, Phase: "archiving", Receipt: *main.Receipt}
		for _, name := range slotResetNames() {
			digest, checkErr := slotResetDigest(ctx, filepath.Join(parent, name))
			if checkErr != nil && !errors.Is(checkErr, os.ErrNotExist) {
				return result, checkErr
			}
			intent.Members = append(intent.Members, slotResetMember{Name: name, Digest: digest})
		}
		if err = saveSlotResetIntent(ctx, intentPath, intent, true); err != nil {
			return result, err
		}
	} else if err != nil {
		return result, err
	} else if err = json.Unmarshal(raw, &intent); err != nil {
		return result, fmt.Errorf("%w: invalid slot reset intent", ErrInstallation)
	}
	if !validSlotResetIntent(intent, parent, archive, version, *main.Receipt) {
		return result, fmt.Errorf("%w: slot reset intent does not match this installation", ErrConflict)
	}
	if err = os.Mkdir(archive, 0700); err != nil && !errors.Is(err, os.ErrExist) {
		return result, err
	}
	if err = plainSlotDirectory(archive); err != nil {
		return result, err
	}
	if err = checkSlotResetArchiveNames(archive, intent.Members); err != nil {
		return result, err
	}

	// Validate every member before the first move. A missing original remains
	// missing; an existing original must be unchanged on exactly one side.
	for _, member := range intent.Members {
		location := filepath.Join(archive, member.Name)
		if intent.Phase == "archiving" {
			source := filepath.Join(parent, member.Name)
			if member.Digest == "" {
				for _, path := range []string{source, location} {
					if _, e := os.Lstat(path); !errors.Is(e, os.ErrNotExist) {
						if e != nil {
							return result, e
						}
						return result, fmt.Errorf("%w: previously absent slot member appeared", ErrConflict)
					}
				}
				continue
			}
			location, _, err = removalLocation(source, location)
			if err != nil {
				return result, err
			}
		}
		digest, checkErr := slotResetDigest(ctx, location)
		if member.Digest == "" && errors.Is(checkErr, os.ErrNotExist) {
			continue
		}
		if checkErr != nil {
			return result, checkErr
		}
		if digest != member.Digest {
			return result, fmt.Errorf("%w: original slot member changed: %s", ErrConflict, member.Name)
		}
	}
	if intent.Phase == "archiving" {
		for _, member := range intent.Members {
			if member.Digest != "" {
				if err = moveRemovalMember(ctx, filepath.Join(parent, member.Name), filepath.Join(archive, member.Name)); err != nil {
					return result, err
				}
			}
		}
		intent.Phase = "installing"
		if err = saveSlotResetIntent(ctx, intentPath, intent, false); err != nil {
			return result, err
		}
	}
	if intent.Phase == "installing" {
		if _, err = installSlotPool(ctx, parent, version); err != nil {
			return result, err
		}
		intent.Phase = "complete"
		if err = saveSlotResetIntent(ctx, intentPath, intent, false); err != nil {
			return result, err
		}
	}
	if _, err = InspectSlots(ctx, parent, version); err != nil {
		return result, err
	}
	result.Complete = true
	return result, nil
}

func slotResetNames() []string {
	names := make([]string, 0, bridge.SlotCount+1)
	for i := 1; i <= bridge.SlotCount; i++ {
		names = append(names, filepath.Base(SlotDirectory("", i)))
	}
	return append(names, slotMarker)
}

func validSlotResetIntent(intent slotResetIntent, parent, archive, version string, receipt InstallationReceipt) bool {
	if intent.Schema != "lycheedev.slot-reset.v1" || intent.Parent != parent || intent.Archive != archive || intent.Version != version || !sameReceipt(intent.Receipt, receipt) {
		return false
	}
	if intent.Phase != "archiving" && intent.Phase != "installing" && intent.Phase != "complete" {
		return false
	}
	names := slotResetNames()
	if len(intent.Members) != len(names) {
		return false
	}
	for i, member := range intent.Members {
		if member.Name != names[i] {
			return false
		}
		if member.Digest != "" {
			decoded, err := hex.DecodeString(member.Digest)
			if err != nil || len(decoded) != sha256.Size || strings.ToLower(member.Digest) != member.Digest {
				return false
			}
		}
	}
	return true
}

func checkSlotResetArchiveNames(archive string, members []slotResetMember) error {
	allowed := make(map[string]bool, len(members))
	for _, member := range members {
		if member.Digest != "" {
			allowed[member.Name] = true
		}
	}
	entries, err := os.ReadDir(archive)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if !allowed[entry.Name()] {
			return fmt.Errorf("%w: unexpected slot archive member", ErrConflict)
		}
	}
	return nil
}

// Hash arbitrary original file/directory contents without decoding payloads or
// following links. Renaming the whole fixed member preserves even corrupt data.
func slotResetDigest(ctx context.Context, path string) (string, error) {
	hash := sha256.New()
	err := filepath.WalkDir(path, func(current string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() && !info.IsDir() {
			return fmt.Errorf("%w: redirected or special slot reset member", ErrInstallation)
		}
		rel, err := filepath.Rel(path, current)
		if err != nil {
			return err
		}
		fmt.Fprintf(hash, "%d:%s:%d:", len(rel), rel, info.Mode().Type())
		if info.IsDir() {
			return plainSlotDirectory(current)
		}
		fmt.Fprintf(hash, "%d:", info.Size())
		file, err := os.Open(current)
		if err != nil {
			return err
		}
		_, readErr := io.Copy(hash, &cancelReader{ctx: ctx, reader: file})
		return errors.Join(readErr, file.Close())
	})
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("%x", hash.Sum(nil)), nil
}

func saveSlotResetIntent(ctx context.Context, path string, intent slotResetIntent, create bool) (err error) {
	raw, err := json.MarshalIndent(intent, "", "  ")
	if err != nil {
		return err
	}
	if !create {
		return vault.ReplaceFile(ctx, path, raw)
	}
	// Publish the first complete intent without replacing an archive transaction
	// created concurrently for a different installation.
	file, err := os.CreateTemp(filepath.Dir(path), ".lycheedev-slot-reset-*")
	if err != nil {
		return err
	}
	temporary := file.Name()
	defer func() {
		if removeErr := os.Remove(temporary); !errors.Is(removeErr, os.ErrNotExist) {
			err = errors.Join(err, removeErr)
		}
	}()
	_, writeErr := file.Write(raw)
	err = errors.Join(writeErr, file.Sync(), file.Close())
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	return publishDirectory(temporary, path)
}
