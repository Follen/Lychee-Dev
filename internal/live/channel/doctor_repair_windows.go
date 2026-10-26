//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrDoctorUnhealthy = errors.New("doctor.check_failed")

type DoctorRepairOptions struct {
	Release       string
	Archive       string
	Request       string
	HardReset     bool
	session       string
	repairArchive *string
}

// RepairDoctor diagnoses and repairs one fixed target through the same managed
// installation and connection recovery paths as explicit Live operations.
func (p *Project) RepairDoctor(ctx context.Context, target TargetRequest, session string, opts DoctorRepairOptions) (DoctorResult, error) {
	report := p.Doctor(ctx, target, session)
	opts.session = session
	var repairArchive string
	opts.repairArchive = &repairArchive
	result, err := p.repairDoctor(ctx, opts, report)
	if result.Session != "" {
		report.Connection = &result
	}
	if err == nil {
		report = p.Doctor(ctx, target, result.Session)
		if result.Session != "" {
			report.Connection = &result
		}
	} else {
		next := "resolve the repair error, then run doctor --fix for the same target"
		if result.Session != "" {
			next = "live resume " + result.Session + " --project <original-project>"
		}
		report.add("live.repair", "blocked", err.Error(), next)
	}
	report.RepairArchive = repairArchive
	if err != nil {
		return report, err
	}
	if !report.Healthy {
		return report, ErrDoctorUnhealthy
	}
	return report, nil
}

func (p *Project) repairDoctor(ctx context.Context, opts DoctorRepairOptions, report DoctorResult) (ProjectResult, error) {
	if report.Target == nil || report.Deployment == nil {
		return ProjectResult{}, ErrDoctorUnhealthy
	}
	target, deployment := report.Target, report.Deployment
	resetPath, client, err := p.doctorResetPath(target.Client.Directory)
	if err != nil {
		return ProjectResult{}, err
	}
	pendingReset, err := readDoctorResetReference(resetPath, client, deployment.Version, opts.Archive)
	if pendingReset != nil && opts.repairArchive != nil {
		*opts.repairArchive = strings.TrimSuffix(pendingReset.Archive, ".slots")
	}
	if err != nil {
		return ProjectResult{}, err
	}
	slotsReady := deployment.Slots != nil && deployment.Slots.Count == bridge.SlotCount && (deployment.Slots.State == "managed" || deployment.Slots.State == "pending")
	needsInstall := deployment.State != "managed" || deployment.Version != buildinfo.Version || deployment.Slots == nil
	// A nil assessment can be an older addon without the slot runtime. That
	// addon takes the normal upgrade path; resetting requires a slot receipt.
	needsReset := pendingReset != nil || (deployment.State == "managed" && deployment.Slots != nil && (opts.HardReset || !slotsReady))
	needsChange := needsInstall || needsReset || opts.Release != ""
	if report.Owner != nil {
		if opts.session != "" && opts.session != report.Owner.OperationID {
			return ProjectResult{}, errors.New("live.channel_owner_mismatch: repair must use the original CON")
		}
		if !needsChange {
			return p.Resume(ctx, report.Owner.OperationID)
		}
		closeOriginal := p.CloseForRepair
		if needsReset {
			closeOriginal = p.closeForSlotRepair
		}
		if result, err := closeOriginal(ctx, report.Owner.OperationID); err != nil {
			return result, err
		}
	}
	if needsChange {
		release := opts.Release
		if release == "" && needsInstall {
			release = bundledDoctorRelease()
		}
		if release == "" && needsInstall {
			return ProjectResult{}, errors.New("live.channel_release_required: supply --release with the current distribution")
		}
		archive := ""
		if !needsReset && deployment.State != "absent" && release != "" {
			var err error
			archive, err = p.doctorArchive(target.Client.Directory, opts.Archive)
			if err != nil {
				return ProjectResult{}, err
			}
		}
		if needsReset {
			// Retain the original manifest and every slot member before publishing
			// a fresh pool. The reset uses the installed main addon's version so
			// any following release upgrade remains a normal managed transition.
			slotArchive, err := p.resetDoctorSlots(ctx, client, deployment.Version, opts.Archive, delivery.ResetAddonSlots)
			archive = strings.TrimSuffix(slotArchive, ".slots")
			if opts.repairArchive != nil {
				*opts.repairArchive = archive
			}
			if err != nil {
				return ProjectResult{}, err
			}
		}
		var err error
		if deployment.State == "absent" {
			_, err = delivery.InstallAddon(ctx, release, target.Client.Directory, buildinfo.Version)
		} else if release != "" {
			_, err = delivery.UpgradeAddon(ctx, release, target.Client.Directory, archive, buildinfo.Version, false)
		}
		if err != nil {
			return ProjectResult{}, err
		}
		key := opts.Request
		if key == "" {
			key = "doctor-" + time.Now().UTC().Format("20060102T150405.000000000")
		}
		return p.Activate(ctx, TargetRequest{Installation: target.Client.Directory, PID: target.Window.ProcessID}, key)
	}
	if opts.session != "" {
		return p.Resume(ctx, opts.session)
	}
	return p.Connect(ctx, TargetRequest{Installation: target.Client.Directory, PID: target.Window.ProcessID})
}

func (p *Project) doctorArchive(clientDirectory, archive string) (string, error) {
	if archive != "" {
		return archive, nil
	}
	parent := filepath.Join(p.Root, ".lycheedev", "live", "repairs")
	if !strings.EqualFold(filepath.VolumeName(p.Root), filepath.VolumeName(clientDirectory)) {
		// Managed replacement renames directories. Cross-volume projects keep
		// their recovery archive on the installation volume outside AddOns.
		parent = filepath.Join(filepath.Dir(filepath.Dir(delivery.AddonDirectory(clientDirectory))), ".lycheedev-repairs")
	}
	archive = filepath.Join(parent, time.Now().UTC().Format("20060102T150405.000000000"))
	if err := os.MkdirAll(filepath.Dir(archive), 0700); err != nil {
		return "", err
	}
	return archive, nil
}

type doctorResetReference struct {
	Schema  string `json:"schema"`
	Client  string `json:"client"`
	Version string `json:"version"`
	Archive string `json:"archive"`
}

type doctorResetFunc func(context.Context, string, string, string) (delivery.SlotReset, error)

func (p *Project) resetDoctorSlots(ctx context.Context, client, version, explicit string, reset doctorResetFunc) (string, error) {
	path, client, err := p.doctorResetPath(client)
	if err != nil {
		return "", err
	}
	lease, err := vault.AcquireLease(ctx, p.path("repairs", ".locks"), filepath.Base(path))
	if err != nil {
		return "", err
	}
	defer lease.Close()
	ref, err := readDoctorResetReference(path, client, version, explicit)
	if err != nil {
		if ref != nil {
			return ref.Archive, err
		}
		return "", err
	}
	if ref == nil {
		archive, err := p.doctorArchive(client, explicit)
		if err != nil {
			return "", err
		}
		archive, err = filepath.Abs(archive)
		if err != nil {
			return "", err
		}
		ref = &doctorResetReference{Schema: "lycheedev.doctor-slot-reset.v1", Client: client, Version: version, Archive: archive + ".slots"}
		if err := writeProjectJSON(ctx, path, ref); err != nil {
			return ref.Archive, err
		}
	}
	result, err := reset(ctx, client, ref.Archive, version)
	if err != nil {
		return ref.Archive, err
	}
	if !result.Complete {
		return ref.Archive, ErrPending
	}
	if err := ctx.Err(); err != nil {
		return ref.Archive, err
	}
	return ref.Archive, os.Remove(path)
}

func (p *Project) doctorResetPath(client string) (string, string, error) {
	client, err := filepath.EvalSymlinks(client)
	if err != nil {
		return "", "", err
	}
	client, err = filepath.Abs(client)
	if err != nil {
		return "", "", err
	}
	digest := sha256.Sum256([]byte(strings.ToLower(client)))
	return p.path("repairs", fmt.Sprintf("%x.pending-reset.json", digest)), client, nil
}

func readDoctorResetReference(path, client, version, explicit string) (*doctorResetReference, error) {
	var ref doctorResetReference
	if err := readProjectJSON(path, &ref, 16384); errors.Is(err, os.ErrNotExist) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	if ref.Schema != "lycheedev.doctor-slot-reset.v1" || !strings.EqualFold(ref.Client, client) || ref.Version != version || !filepath.IsAbs(ref.Archive) || !strings.HasSuffix(ref.Archive, ".slots") {
		return &ref, fmt.Errorf("%w: pending doctor slot reset does not match this installation and version", delivery.ErrConflict)
	}
	if explicit != "" {
		archive, err := filepath.Abs(explicit)
		if err != nil {
			return &ref, err
		}
		if !strings.EqualFold(archive+".slots", ref.Archive) {
			return &ref, fmt.Errorf("%w: --output differs from the pending doctor slot reset archive", delivery.ErrConflict)
		}
	}
	return &ref, nil
}

func bundledDoctorRelease() string {
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	directory := filepath.Dir(exe)
	for depth := 0; depth < 3; depth++ {
		if info, err := os.Stat(filepath.Join(directory, "release.json")); err == nil && info.Mode().IsRegular() {
			return directory
		}
		directory = filepath.Dir(directory)
	}
	return ""
}
