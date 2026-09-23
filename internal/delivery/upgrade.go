package delivery

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

type Upgrade struct {
	Target  string              `json:"target"`
	Archive string              `json:"archive"`
	Receipt InstallationReceipt `json:"receipt"`
}

type replacementRecord struct {
	Schema   string              `json:"schema"`
	Target   string              `json:"target"`
	Previous InstallationReceipt `json:"previous"`
	Next     InstallationReceipt `json:"next"`
}

// UpgradeInstallation retains the old tree in archive/previous. archive must be
// an absent directory outside the target's discovery parent. A journal written
// before either move enables ResumeUpgrade after a process interruption. It is
// not a power-loss durability guarantee or authorization to replace edited files.
func UpgradeInstallation(ctx context.Context, releaseDirectory, target, archive, component, version string) (Upgrade, error) {
	if component != "addon" && component != "skill" {
		return Upgrade{}, ErrInstallation
	}
	parent, target, err := installationDestination(target)
	if err != nil {
		return Upgrade{}, err
	}
	_, archive, err = installationDestination(archive)
	if err != nil {
		return Upgrade{}, err
	}
	if err := outsideDiscovery(parent, archive); err != nil {
		return Upgrade{}, err
	}
	scope, resource := installationLeaseIdentity(parent, target)
	lease, err := vault.AcquireLease(ctx, scope, resource)
	if err != nil {
		return Upgrade{}, err
	}
	defer lease.Close()
	current, err := InspectInstallation(ctx, target, component)
	if err != nil {
		return Upgrade{}, err
	}
	if current.State != "managed" {
		return Upgrade{}, fmt.Errorf("%w: %s", ErrConflict, current.State)
	}
	if err := os.Mkdir(archive, 0700); err != nil {
		return Upgrade{}, err
	}
	// Retain an incomplete preparation on failure. No installed bytes have moved
	// yet; never infer ownership of an existing archive on retry.
	next, err := InstallFresh(ctx, releaseDirectory, filepath.Join(archive, "next"), component, version)
	if err != nil {
		return Upgrade{}, fmt.Errorf("prepare %s: %w", archive, err)
	}
	record := replacementRecord{Schema: "lycheedev.replacement.v1", Target: target, Previous: *current.Receipt, Next: next}
	raw, err := json.Marshal(record)
	if err != nil {
		return Upgrade{}, err
	}
	file, err := os.OpenFile(filepath.Join(archive, "replacement.json"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return Upgrade{}, err
	}
	_, writeErr := file.Write(raw)
	syncErr := file.Sync()
	closeErr := file.Close()
	if err := errors.Join(writeErr, syncErr, closeErr); err != nil {
		return Upgrade{}, err
	}
	return resumeReplacement(ctx, target, archive, record)
}

// ResumeUpgrade requires the explicit original target and archive, and never
// accepts a destination from the journal as an instruction. New release files
// are unnecessary: the staged tree and both receipts are verified again.
func ResumeUpgrade(ctx context.Context, target, archive, component string) (Upgrade, error) {
	if component != "addon" && component != "skill" {
		return Upgrade{}, ErrInstallation
	}
	parent, target, err := installationDestination(target)
	if err != nil {
		return Upgrade{}, err
	}
	_, archive, err = installationDestination(archive)
	if err != nil {
		return Upgrade{}, err
	}
	if err := outsideDiscovery(parent, archive); err != nil {
		return Upgrade{}, err
	}
	scope, resource := installationLeaseIdentity(parent, target)
	lease, err := vault.AcquireLease(ctx, scope, resource)
	if err != nil {
		return Upgrade{}, err
	}
	defer lease.Close()
	info, err := os.Lstat(archive)
	if err != nil {
		return Upgrade{}, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return Upgrade{}, ErrInstallation
	}
	root, err := os.OpenRoot(archive)
	if err != nil {
		return Upgrade{}, err
	}
	defer root.Close()
	info, err = root.Lstat("replacement.json")
	if err != nil {
		return Upgrade{}, err
	}
	if !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return Upgrade{}, ErrInstallation
	}
	file, err := root.Open("replacement.json")
	if err != nil {
		return Upgrade{}, err
	}
	defer file.Close()
	raw, err := io.ReadAll(&cancelReader{ctx: ctx, reader: io.LimitReader(file, (1<<20)+1)})
	if err != nil {
		return Upgrade{}, err
	}
	if len(raw) > 1<<20 || !utf8.Valid(raw) {
		return Upgrade{}, ErrInstallation
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err := uniqueJSON(decoder, 0); err != nil {
		return Upgrade{}, fmt.Errorf("%w: %v", ErrInstallation, err)
	}
	if _, err := decoder.Token(); err != io.EOF {
		return Upgrade{}, ErrInstallation
	}
	decoder = json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var record replacementRecord
	if err := decoder.Decode(&record); err != nil {
		return Upgrade{}, fmt.Errorf("%w: %v", ErrInstallation, err)
	}
	if record.Next.Component != component {
		return Upgrade{}, ErrInstallation
	}
	return resumeReplacement(ctx, target, archive, record)
}

func resumeReplacement(ctx context.Context, target, archive string, record replacementRecord) (Upgrade, error) {
	component := record.Next.Component
	if record.Schema != "lycheedev.replacement.v1" || record.Target != target || (component != "addon" && component != "skill") || record.Previous.Component != component {
		return Upgrade{}, ErrInstallation
	}
	current, err := InspectInstallation(ctx, target, component)
	if err != nil {
		return Upgrade{}, err
	}
	previous, err := InspectInstallation(ctx, filepath.Join(archive, "previous"), component)
	if err != nil {
		return Upgrade{}, err
	}
	next, err := InspectInstallation(ctx, filepath.Join(archive, "next"), component)
	if err != nil {
		return Upgrade{}, err
	}
	oldValid := previous.State == "managed" && sameReceipt(*previous.Receipt, record.Previous)
	if oldValid && current.State == "managed" && sameReceipt(*current.Receipt, record.Next) && next.State == "absent" {
		return Upgrade{Target: target, Archive: archive, Receipt: record.Next}, nil
	}
	if next.State != "managed" || !sameReceipt(*next.Receipt, record.Next) {
		return Upgrade{}, fmt.Errorf("%w: prepared content changed", ErrConflict)
	}
	if previous.State == "absent" && current.State == "managed" && sameReceipt(*current.Receipt, record.Previous) {
		if err := ctx.Err(); err != nil {
			return Upgrade{}, err
		}
		if err := publishDirectory(target, filepath.Join(archive, "previous")); err != nil {
			return Upgrade{}, err
		}
		oldValid = true
		current.State = "absent"
	}
	if !oldValid || current.State != "absent" {
		return Upgrade{}, fmt.Errorf("%w: replacement state is ambiguous", ErrConflict)
	}
	if err := ctx.Err(); err != nil {
		return Upgrade{}, err
	}
	if err := publishDirectory(filepath.Join(archive, "next"), target); err != nil {
		return Upgrade{}, err
	}
	return Upgrade{Target: target, Archive: archive, Receipt: record.Next}, nil
}

func outsideDiscovery(parent, archive string) error {
	rel, err := filepath.Rel(strings.ToLower(parent), strings.ToLower(archive))
	if err != nil {
		return err
	}
	if rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("%w: archive must be outside the installation discovery directory", ErrInstallation)
	}
	return nil
}
