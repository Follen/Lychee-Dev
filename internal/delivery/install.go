package delivery

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrConflict = errors.New("delivery.installation_conflict")

// InstallFresh installs into an absent target, or returns the identical verified
// installation on retry. Existing installations of another release require the
// upgrade transaction, not this operation. Parent must already exist. The shared
// parent-scoped OS lease does not depend on the caller's workspace.
func InstallFresh(ctx context.Context, releaseDirectory, target, component, version string) (receipt InstallationReceipt, err error) {
	if component != "addon" && component != "skill" {
		return receipt, ErrInstallation
	}
	release, err := InspectRelease(ctx, releaseDirectory, version)
	if err != nil {
		return receipt, err
	}
	parent, target, err := installationDestination(target)
	if err != nil {
		return receipt, err
	}
	wanted := InstallationReceipt{Schema: "lycheedev.installation.v1", Component: component, Version: release.Version, Commit: release.Commit}
	for _, resource := range release.Resources {
		if strings.HasPrefix(resource.Path, component+"/") {
			wanted.Resources = append(wanted.Resources, resource)
		}
	}
	stage, err := StagePayload(ctx, filepath.Join(releaseDirectory, "payload"), parent, release.Resources)
	if err != nil {
		return receipt, err
	}
	defer func() {
		if filepath.Dir(stage) != parent || !strings.HasPrefix(filepath.Base(stage), ".lycheedev-stage-") {
			err = errors.Join(err, ErrInstallation)
			return
		}
		err = errors.Join(err, os.RemoveAll(stage))
	}()
	prepared := filepath.Join(stage, component)
	raw, err := json.Marshal(wanted)
	if err != nil {
		return receipt, err
	}
	marker, err := os.OpenFile(filepath.Join(prepared, installationMarker), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return receipt, err
	}
	_, writeErr := marker.Write(raw)
	syncErr := marker.Sync()
	closeErr := marker.Close()
	if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
		return receipt, err
	}
	check, err := InspectInstallation(ctx, prepared, component)
	if err != nil {
		return receipt, err
	}
	if check.State != "managed" {
		return receipt, fmt.Errorf("%w: staged content", ErrInstallation)
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(parent, ".lycheedev-locks"), "installation:"+strings.ToLower(target))
	if err != nil {
		return receipt, err
	}
	defer lease.Close()
	current, err := InspectInstallation(ctx, target, component)
	if err != nil {
		return receipt, err
	}
	if current.State == "managed" && sameReceipt(*current.Receipt, wanted) {
		return wanted, nil
	}
	if current.State != "absent" {
		return receipt, fmt.Errorf("%w: %s", ErrConflict, current.State)
	}
	if err = ctx.Err(); err != nil {
		return receipt, err
	}
	// OS-level no-replace protects even against a non-cooperating creator after
	// the assessment. Never fall back to a rename that can replace an empty dir.
	if err = publishDirectory(prepared, target); err != nil {
		return receipt, err
	}
	return wanted, nil
}

func sameReceipt(a, b InstallationReceipt) bool {
	if a.Schema != b.Schema || a.Component != b.Component || a.Version != b.Version || a.Commit != b.Commit || len(a.Resources) != len(b.Resources) {
		return false
	}
	files := make(map[string]Resource, len(a.Resources))
	for _, r := range a.Resources {
		files[r.Path] = r
	}
	for _, r := range b.Resources {
		if files[r.Path] != r {
			return false
		}
	}
	return true
}

func installationDestination(target string) (parent, destination string, err error) {
	abs, err := filepath.Abs(target)
	if err != nil {
		return "", "", err
	}
	parent, err = filepath.EvalSymlinks(filepath.Dir(abs))
	if err != nil {
		return "", "", err
	}
	name := filepath.Base(abs)
	if name == "." || name == string(filepath.Separator) || strings.HasPrefix(strings.ToLower(name), ".lycheedev-") {
		return "", "", ErrInstallation
	}
	return parent, filepath.Join(parent, name), nil
}
