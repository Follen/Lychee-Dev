package delivery

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// UpdateTarget contains a skill directory or a verified game client directory.
// The saved inventory is discovery data, never permission to replace edited files.
type UpdateTarget struct {
	Component string `json:"component"`
	Path      string `json:"path"`
}

type UpdatedTarget struct {
	UpdateTarget
	State           string `json:"state"`
	PreviousVersion string `json:"previousVersion,omitempty"`
	Recovery        string `json:"recovery,omitempty"`
	Activation      string `json:"activation,omitempty"`
}

type UpdateResult struct {
	Version  string          `json:"version"`
	Commit   string          `json:"commit"`
	Complete bool            `json:"complete"`
	Plan     bool            `json:"plan"`
	Targets  []UpdatedTarget `json:"targets"`
}

func ReadUpdateTargets(state string) ([]UpdateTarget, error) {
	file := filepath.Join(state, "targets.json")
	info, err := os.Lstat(file)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > 64<<10 {
		return nil, ErrInstallation
	}
	raw, err := os.ReadFile(file)
	if err != nil {
		return nil, err
	}
	var targets []UpdateTarget
	if err := json.Unmarshal(raw, &targets); err != nil {
		return nil, err
	}
	if len(targets) > 64 {
		return nil, ErrInstallation
	}
	return targets, nil
}

// DiscoverUpdateTargets treats the inventory as remembered locations, not a
// standing request to reinstall removed targets. A retained replacement journal
// still selects its original target when a crash has moved the installed tree.
func DiscoverUpdateTargets(ctx context.Context, state string) ([]UpdateTarget, error) {
	targets, err := ReadUpdateTargets(state)
	if err != nil {
		return nil, err
	}
	result := make([]UpdateTarget, 0, len(targets))
	for _, target := range targets {
		directory := target.Path
		if target.Component == "addon" {
			directory = AddonDirectory(directory)
		}
		current, err := InspectInstallation(ctx, directory, target.Component)
		if err != nil {
			return nil, err
		}
		if current.State != "absent" {
			result = append(result, target)
			continue
		}
		recoveries, err := pendingUpdateRecoveries(directory, target.Component)
		if err != nil {
			return nil, err
		}
		if len(recoveries) != 0 {
			result = append(result, target)
		}
	}
	return result, nil
}

// UpdateManaged checks every selected target before changing the first one.
// Each replacement uses the existing locked, resumable installer. Successful
// replacements discard their verified old tree; interrupted replacements retain
// their transaction until the same update is retried.
func UpdateManaged(ctx context.Context, releaseDirectory, version, state string, targets []UpdateTarget, plan bool) (result UpdateResult, err error) {
	result.Plan, result.Targets = plan, []UpdatedTarget{}
	release, err := InspectRelease(ctx, releaseDirectory, version)
	if err != nil {
		return result, err
	}
	result.Version, result.Commit = release.Version, release.Commit
	state, err = updateDestination(state)
	if err != nil {
		return result, err
	}
	if len(targets) > 64 {
		return result, fmt.Errorf("%w: update supports at most 64 targets", ErrInstallation)
	}
	if len(targets) == 0 {
		result.Complete = !plan
		return result, nil
	}
	seen := map[string]bool{}
	for _, target := range targets {
		if strings.TrimSpace(target.Path) == "" || target.Component != "skill" && target.Component != "addon" {
			return result, ErrInstallation
		}
		absolute, err := updateDestination(target.Path)
		if err != nil {
			return result, err
		}
		target.Path = filepath.Clean(absolute)
		directory := target.Path
		if target.Component == "addon" {
			deployment, parent, err := ResolveAddonDestination(ctx, target.Path)
			if err != nil {
				return result, err
			}
			target.Path = deployment.Client.Directory
			directory = AddonDirectory(target.Path)
			if _, err := InspectAddonRelease(ctx, releaseDirectory, version); err != nil {
				return result, err
			}
			gate, err := journal.AcquireInstallationMaintenance(ctx, parent)
			if err != nil {
				return result, err
			}
			if err := gate.Close(); err != nil {
				return result, err
			}
		} else if filepath.Base(target.Path) != "lycheedev" {
			return result, ErrInstallation
		}
		// A game root is only a selector. Identity, de-duplication and recovery
		// are all based on the resolved client, just like the saved inventory.
		key := strings.ToLower(target.Component + ":" + target.Path)
		if seen[key] {
			continue
		}
		seen[key] = true
		if _, _, err := installationDestination(directory); err != nil {
			return result, err
		}
		current, err := InspectInstallation(ctx, directory, target.Component)
		if err != nil {
			return result, err
		}
		entry := UpdatedTarget{UpdateTarget: target, State: current.State}
		if current.Receipt != nil {
			entry.PreviousVersion = current.Receipt.Version
		}
		digest := sha256.Sum256([]byte(key + ":" + release.Version + ":" + release.Commit))
		// Renames must stay on the target volume (a game often lives on D:
		// while the Agent/workspace lives on C:). Keep recovery outside discovery.
		entry.Recovery = filepath.Join(filepath.Dir(filepath.Dir(directory)), ".lycheedev-updates", fmt.Sprintf("replace-%x", digest[:16]))
		// Older callers hashed the game-root selector. Keep their original
		// journal/cleanup directory rather than creating a second transaction.
		recoveries, err := pendingUpdateRecoveries(directory, target.Component)
		if err != nil {
			return result, err
		}
		if len(recoveries) > 1 {
			return result, fmt.Errorf("%w: multiple update recoveries for %s", ErrConflict, directory)
		}
		if len(recoveries) == 1 {
			entry.Recovery = recoveries[0].archive
			if !sameReceipt(recoveries[0].record.Next, updateReceipt(release, target.Component)) {
				result.Targets = append(result.Targets, entry)
				return result, fmt.Errorf("%w: resume the original update at %s", ErrConflict, entry.Recovery)
			}
		}
		if current.State != "managed" && current.State != "absent" {
			result.Targets = append(result.Targets, entry)
			return result, fmt.Errorf("%w: %s: %s", ErrConflict, directory, current.State)
		}
		if strings.EqualFold(filepath.VolumeName(directory), filepath.VolumeName(state)) {
			if err := outsideDiscovery(filepath.Dir(directory), state); err != nil {
				return result, err
			}
		}
		result.Targets = append(result.Targets, entry)
	}
	if plan {
		return result, nil
	}
	if err := os.MkdirAll(state, 0700); err != nil {
		return result, err
	}
	canonicalState, err := filepath.EvalSymlinks(state)
	if err != nil {
		return result, err
	}
	absoluteState, err := filepath.Abs(state)
	if err != nil || !strings.EqualFold(canonicalState, absoluteState) {
		return result, ErrInstallation
	}
	lease, err := vault.AcquireLease(ctx, state, "managed-update")
	if err != nil {
		return result, err
	}
	defer lease.Close()
	// Persist the union before mutation, so a subsequent default update can find
	// custom targets even when the process stops between independent components.
	remembered, err := ReadUpdateTargets(state)
	if err != nil {
		return result, err
	}
	for _, item := range result.Targets {
		found := false
		for _, previous := range remembered {
			if previous.Component == item.Component && strings.EqualFold(previous.Path, item.Path) {
				found = true
			}
		}
		if !found {
			remembered = append(remembered, item.UpdateTarget)
		}
	}
	if len(remembered) > 64 {
		return result, ErrInstallation
	}
	raw, err := json.Marshal(remembered)
	if err != nil {
		return result, err
	}
	if err := writeUpdateInventory(state, raw); err != nil {
		return result, err
	}
	for i := range result.Targets {
		item := &result.Targets[i]
		wanted := updateReceipt(release, item.Component)
		if err := ctx.Err(); err != nil {
			return result, err
		}
		directory := item.Path
		if item.Component == "addon" {
			directory = AddonDirectory(item.Path)
		}
		_, recoveryErr := os.Lstat(item.Recovery)
		if recoveryErr != nil && !errors.Is(recoveryErr, os.ErrNotExist) {
			return result, recoveryErr
		}
		_, cleanupErr := os.Lstat(item.Recovery + ".cleanup.json")
		if cleanupErr != nil && !errors.Is(cleanupErr, os.ErrNotExist) {
			return result, cleanupErr
		}
		if cleanupErr == nil {
			err = discardReplacement(ctx, directory, item.Recovery, wanted)
		} else if recoveryErr == nil {
			if item.Component == "addon" {
				_, err = UpgradeAddon(ctx, "", item.Path, item.Recovery, version, true)
			} else {
				_, err = UpgradeSkill(ctx, "", item.Path, item.Recovery, version, true)
			}
		} else if item.State == "absent" {
			if item.Component == "addon" {
				_, err = InstallAddon(ctx, releaseDirectory, item.Path, version)
			} else {
				_, err = InstallSkill(ctx, releaseDirectory, item.Path, version)
			}
		} else {
			// InstallFresh checks full content equality under the installation lock;
			// an identical update is a verified no-op, not a needless directory swap.
			if item.Component == "addon" {
				_, err = InstallAddon(ctx, releaseDirectory, item.Path, version)
			} else {
				_, err = InstallSkill(ctx, releaseDirectory, item.Path, version)
			}
			if errors.Is(err, ErrConflict) {
				if err := os.MkdirAll(filepath.Dir(item.Recovery), 0700); err != nil {
					return result, err
				}
				if item.Component == "addon" {
					_, err = UpgradeAddon(ctx, releaseDirectory, item.Path, item.Recovery, version, false)
				} else {
					_, err = UpgradeSkill(ctx, releaseDirectory, item.Path, item.Recovery, version, false)
				}
			}
		}
		if err != nil {
			item.State = "pending"
			return result, err
		}
		if _, err = os.Lstat(item.Recovery); err == nil {
			if err = discardReplacement(ctx, directory, item.Recovery, wanted); err != nil {
				item.State = "cleanup_pending"
				return result, err
			}
		} else if !errors.Is(err, os.ErrNotExist) {
			return result, err
		}
		item.State, item.Recovery = "current", ""
		if item.Component == "addon" {
			item.Activation = "reload_required"
		} else {
			item.Activation = "new_agent_context_required"
		}
	}
	result.Complete = true
	return result, nil
}

// Retained records are discovery evidence only: the installer still verifies
// the selected target, both receipts and the staged files under its usual lock.
// Include cleanup markers because a crash may already have removed the archive.
type updateRecovery struct {
	archive string
	record  replacementRecord
}

func pendingUpdateRecoveries(directory, component string) ([]updateRecovery, error) {
	root := filepath.Join(filepath.Dir(filepath.Dir(directory)), ".lycheedev-updates")
	paths, err := filepath.Glob(filepath.Join(root, "replace-*", "replacement.json"))
	if err != nil {
		return nil, err
	}
	markers, err := filepath.Glob(filepath.Join(root, "replace-*.cleanup.json"))
	if err != nil {
		return nil, err
	}
	seen := map[string]bool{}
	var result []updateRecovery
	for _, path := range append(markers, paths...) {
		raw, err := readSlotFile(path, 1<<20)
		if err != nil {
			return nil, err
		}
		var record replacementRecord
		if json.Unmarshal(raw, &record) != nil || record.Schema != "lycheedev.replacement.v1" || !strings.EqualFold(record.Target, directory) || record.Next.Component != component {
			continue
		}
		archive := filepath.Dir(path)
		if strings.HasSuffix(path, ".cleanup.json") {
			archive = strings.TrimSuffix(path, ".cleanup.json")
		}
		if !seen[archive] {
			seen[archive] = true
			result = append(result, updateRecovery{archive: archive, record: record})
		}
	}
	return result, nil
}

func updateReceipt(release Release, component string) InstallationReceipt {
	wanted := InstallationReceipt{Schema: "lycheedev.installation.v1", Component: component, Version: release.Version, Commit: release.Commit}
	for _, resource := range release.Resources {
		if strings.HasPrefix(resource.Path, component+"/") {
			wanted.Resources = append(wanted.Resources, resource)
		}
	}
	return wanted
}

// Resolve existing parent aliases (including Windows 8.3 paths), while rejecting
// a redirected leaf. Missing children keep their explicit destination names.
func updateDestination(path string) (string, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	info, err := os.Lstat(absolute)
	if err == nil && info.Mode()&os.ModeSymlink != 0 {
		return "", ErrInstallation
	}
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	cursor, suffix := absolute, []string{}
	for {
		canonical, err := filepath.EvalSymlinks(cursor)
		if err == nil {
			for i := len(suffix) - 1; i >= 0; i-- {
				canonical = filepath.Join(canonical, suffix[i])
			}
			return canonical, nil
		}
		if !errors.Is(err, os.ErrNotExist) || filepath.Dir(cursor) == cursor {
			return "", err
		}
		suffix = append(suffix, filepath.Base(cursor))
		cursor = filepath.Dir(cursor)
	}
}

func writeUpdateInventory(state string, raw []byte) error {
	f, err := os.CreateTemp(state, ".targets-")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	_, writeErr := f.Write(raw)
	syncErr, closeErr := f.Sync(), f.Close()
	if err := errors.Join(writeErr, syncErr, closeErr); err != nil {
		return err
	}
	return os.Rename(f.Name(), filepath.Join(state, "targets.json"))
}

func discardReplacement(ctx context.Context, target, archive string, wanted InstallationReceipt) error {
	parent, target, err := installationDestination(target)
	if err != nil {
		return err
	}
	if err := outsideDiscovery(parent, archive); err != nil {
		return err
	}
	scope, resource := installationLeaseIdentity(parent, target)
	lease, err := vault.AcquireLease(ctx, scope, resource)
	if err != nil {
		return err
	}
	defer lease.Close()
	current, err := InspectInstallation(ctx, target, wanted.Component)
	if err != nil {
		return err
	}
	if current.State != "managed" || !sameReceipt(*current.Receipt, wanted) {
		return ErrConflict
	}
	marker := archive + ".cleanup.json"
	markerInfo, markerErr := os.Lstat(marker)
	if markerErr != nil && !errors.Is(markerErr, os.ErrNotExist) {
		return markerErr
	}
	path := marker
	if errors.Is(markerErr, os.ErrNotExist) {
		path = filepath.Join(archive, "replacement.json")
	} else if !markerInfo.Mode().IsRegular() || markerInfo.Size() > 1<<20 {
		return ErrInstallation
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return errors.Join(err, ErrInstallation)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var record replacementRecord
	if err := json.Unmarshal(raw, &record); err != nil {
		return err
	}
	if record.Target != target || record.Schema != "lycheedev.replacement.v1" || !sameReceipt(record.Next, wanted) {
		return ErrInstallation
	}
	if markerErr != nil {
		// Before deletion, both trees must still be intact. Once this durable
		// marker exists, partial old-tree deletion is an expected recovery state.
		if _, err := resumeReplacement(ctx, target, archive, record); err != nil {
			return err
		}
		file, err := os.CreateTemp(filepath.Dir(archive), ".cleanup-")
		if err != nil {
			return err
		}
		defer os.Remove(file.Name())
		_, writeErr := file.Write(raw)
		syncErr, closeErr := file.Sync(), file.Close()
		if err := errors.Join(writeErr, syncErr, closeErr); err != nil {
			return err
		}
		if err := os.Rename(file.Name(), marker); err != nil {
			return err
		}
	}
	if info, err := os.Lstat(archive); err == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return ErrInstallation
		}
		entries, err := os.ReadDir(archive)
		if err != nil {
			return err
		}
		for _, entry := range entries {
			if entry.Name() != "previous" && entry.Name() != "replacement.json" && entry.Name() != ".lycheedev-locks" {
				return ErrConflict
			}
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		// Exact archive, outside discovery; the deletion intent survives it.
		if err := os.RemoveAll(archive); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return os.Remove(marker)
}
