//go:build windows && amd64

package channel

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/testkit"
)

func TestLiveDoctorUpgradeCreatesDefaultArchiveParent(t *testing.T) {
	project := t.TempDir()
	testLiveDoctorArchiveUpgrade(t, testkit.Client(t, "flavor"), project, filepath.Join(project, ".lycheedev", "live", "repairs"), "", "", false)
}

func TestDoctorResetRetainsDefaultArchiveAcrossRestart(t *testing.T) {
	p, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	client := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	var archives []string
	reset := func(_ context.Context, gotClient, archive, version string) (delivery.SlotReset, error) {
		if gotClient != client || version != buildinfo.Version {
			t.Fatalf("changed reset identity: client=%q version=%q", gotClient, version)
		}
		archives = append(archives, archive)
		path, canonicalClient, err := p.doctorResetPath(client)
		if err != nil {
			t.Fatal(err)
		}
		ref, err := readDoctorResetReference(path, canonicalClient, version, "")
		if err != nil || ref == nil || ref.Archive != archive {
			t.Fatalf("reset began before its reference was durable: %+v %v", ref, err)
		}
		if err := os.MkdirAll(archive, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(archive+".slot-reset.json", []byte("retained intent"), 0600); err != nil {
			t.Fatal(err)
		}
		if len(archives) == 1 {
			cancel()
			return delivery.SlotReset{Archive: archive, Complete: true}, nil
		}
		return delivery.SlotReset{Archive: archive, Complete: true}, nil
	}
	first, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, "", reset)
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	// Reopening the project supplies no process-local archive state. The retry
	// must find the original durable reference even if the pool is now ready.
	p, err = OpenProject(p.Root)
	if err != nil {
		t.Fatal(err)
	}
	ctx = context.Background()
	second, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, "", reset)
	if err != nil || first != second || len(archives) != 2 || archives[0] != archives[1] {
		t.Fatalf("default retry changed archives: first=%q second=%q calls=%v error=%v", first, second, archives, err)
	}
	if _, err := os.Stat(first + ".slot-reset.json"); err != nil {
		t.Fatalf("completed repair removed original intent: %v", err)
	}
	path, _, err := p.doctorResetPath(client)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("completed reset retained its pending reference: %v", err)
	}
	third, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, "", reset)
	if err != nil || third == first {
		t.Fatalf("new hard reset reused a completed archive: %q %v", third, err)
	}
}

func TestDoctorRepairResumesPendingResetDespiteReadyAssessment(t *testing.T) {
	p, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	client := testkit.Client(t, "flavor")
	ctx := context.Background()
	archive, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, "", func(_ context.Context, _, archive, _ string) (delivery.SlotReset, error) {
		return delivery.SlotReset{Archive: archive}, context.Canceled
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	// This synthetic assessment represents a pool made ready before the prior
	// reset's final reference cleanup. Missing main files stop the actual reset
	// before disk publication, so no game or full 500-slot fixture is needed.
	report := DoctorResult{Target: &live.ClientWindow{}, Deployment: &DoctorDeployment{State: "managed", Version: buildinfo.Version, Slots: &delivery.SlotAssessment{State: "managed", Count: bridge.SlotCount}}}
	report.Target.Client.Directory = client
	var presentedArchive string
	_, err = p.repairDoctor(ctx, DoctorRepairOptions{repairArchive: &presentedArchive}, report)
	if !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("ready assessment bypassed the original reset: %v", err)
	}
	if presentedArchive != strings.TrimSuffix(archive, ".slots") {
		t.Fatalf("repair did not expose reusable --output: %q", presentedArchive)
	}
}

func TestDoctorResetRejectsChangedOutputAndVersion(t *testing.T) {
	p, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	client := t.TempDir()
	ctx := context.Background()
	first, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, "", func(_ context.Context, _, archive, _ string) (delivery.SlotReset, error) {
		return delivery.SlotReset{Archive: archive}, context.Canceled
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	called := false
	reset := func(context.Context, string, string, string) (delivery.SlotReset, error) {
		called = true
		return delivery.SlotReset{}, nil
	}
	for _, test := range []struct{ name, version, archive string }{
		{"output", buildinfo.Version, filepath.Join(t.TempDir(), "different")},
		{"version", "changed-version", ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			if _, err := p.resetDoctorSlots(ctx, client, test.version, test.archive, reset); !errors.Is(err, delivery.ErrConflict) {
				t.Fatalf("changed pending reset was accepted: %v", err)
			}
			if called {
				t.Fatal("conflicting reset mutated the installation")
			}
		})
	}
	resumed, err := p.resetDoctorSlots(ctx, client, buildinfo.Version, strings.TrimSuffix(first, ".slots"), func(_ context.Context, _, archive, _ string) (delivery.SlotReset, error) {
		return delivery.SlotReset{Archive: archive, Complete: true}, nil
	})
	if err != nil || resumed != first {
		t.Fatalf("conflict changed the pending reference: %q %v", resumed, err)
	}
}

func doctorRepairSlotFixture(t *testing.T, versions ...string) (*Project, DoctorResult, string) {
	t.Helper()
	ctx := context.Background()
	version := buildinfo.Version
	if len(versions) != 0 {
		version = versions[0]
	}
	client := testkit.Client(t, "flavor")
	if _, err := delivery.InstallAddon(ctx, doctorRepairRelease(t, "slot-runtime", version), client, version); err != nil {
		t.Fatal(err)
	}
	deployment, err := delivery.InspectAddonDeployment(ctx, client)
	if err != nil {
		t.Fatal(err)
	}
	p, err := OpenProject(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return p, DoctorResult{Target: &live.ClientWindow{Client: deployment.Client}, Deployment: &DoctorDeployment{State: "managed", Version: version, Slots: deployment.Slots}}, filepath.Join(client, "Interface", "AddOns")
}

func TestDoctorRepairResetsAtInstalledVersionBeforeUpgrade(t *testing.T) {
	const oldVersion = "3.1.1"
	p, report, parent := doctorRepairSlotFixture(t, oldVersion)
	ctx := context.Background()
	marker := filepath.Join(parent, ".lycheedev-slots.json")
	broken := []byte("{corrupt old manifest\x00\xff")
	if err := os.WriteFile(marker, broken, 0600); err != nil {
		t.Fatal(err)
	}
	deployment, err := delivery.InspectAddonDeployment(ctx, report.Target.Client.Directory)
	if err != nil {
		t.Fatal(err)
	}
	report.Deployment.Slots = deployment.Slots
	archive := filepath.Join(t.TempDir(), "repair")
	_, err = p.repairDoctor(ctx, DoctorRepairOptions{Release: testkit.Release(t, "slot-runtime"), Archive: archive}, report)
	if err == nil || err.Error() != "live.channel_explicit_target_required" {
		t.Fatalf("repair failed before safe activation boundary: %v", err)
	}
	pool, err := delivery.InspectSlots(ctx, parent, buildinfo.Version)
	if err != nil || len(pool.Files) != bridge.SlotCount {
		t.Fatalf("upgraded pool: files=%d error=%v", len(pool.Files), err)
	}
	previous, err := delivery.InspectInstallation(ctx, filepath.Join(archive, "previous"), "addon")
	if err != nil || previous.Receipt == nil || previous.Receipt.Version != oldVersion {
		t.Fatalf("original main addon lost: %+v %v", previous, err)
	}
	got, err := os.ReadFile(filepath.Join(archive+".slots", ".lycheedev-slots.json"))
	if err != nil || !bytes.Equal(got, broken) {
		t.Fatalf("old corrupt manifest lost: %q %v", got, err)
	}
}

func TestDoctorRepairInitializesBrokenSlotsAndPreservesOriginalBytes(t *testing.T) {
	for _, state := range []string{"missing_manifest", "corrupt_manifest", "drifted_payload", "hard_reset"} {
		t.Run(state, func(t *testing.T) {
			p, report, parent := doctorRepairSlotFixture(t)
			ctx := context.Background()
			envelope := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Fence: 1, Action: "bind"}
			if err := delivery.PublishSlot(ctx, parent, buildinfo.Version, "retained-original", envelope); err != nil {
				t.Fatal(err)
			}
			marker := filepath.Join(parent, ".lycheedev-slots.json")
			payloadPath := filepath.Join(delivery.SlotDirectory(parent, 1), "Payload.lua")
			switch state {
			case "missing_manifest":
				if err := os.Remove(marker); err != nil {
					t.Fatal(err)
				}
			case "corrupt_manifest":
				if err := os.WriteFile(marker, []byte("{unresolved manifest\x00\xff"), 0600); err != nil {
					t.Fatal(err)
				}
			case "drifted_payload":
				if err := os.WriteFile(payloadPath, []byte("-- unknown payload\x00\xff\n"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			originalMarker, markerErr := os.ReadFile(marker)
			originalPayload, err := os.ReadFile(payloadPath)
			if err != nil {
				t.Fatal(err)
			}
			deployment, err := delivery.InspectAddonDeployment(ctx, report.Target.Client.Directory)
			if err != nil {
				t.Fatal(err)
			}
			report.Deployment.Slots = deployment.Slots
			archive := filepath.Join(t.TempDir(), "repair")
			// PID zero stops before process lookup/input, after the disk repair.
			_, err = p.repairDoctor(ctx, DoctorRepairOptions{Archive: archive, HardReset: state == "hard_reset"}, report)
			if err == nil || err.Error() != "live.channel_explicit_target_required" {
				t.Fatalf("repair failed before safe activation boundary: %v", err)
			}
			pool, err := delivery.InspectSlots(ctx, parent, buildinfo.Version)
			if err != nil || len(pool.Files) != bridge.SlotCount {
				t.Fatalf("fresh pool: files=%d error=%v", len(pool.Files), err)
			}
			for i, slot := range pool.Files {
				if slot.Nonce != "" || slot.PendingHash != "" || slot.Consumer != "" || slot.Runtime != "" {
					t.Fatalf("reset preserved business in slot %d: %+v", i+1, slot)
				}
			}
			retainedPayload, err := os.ReadFile(filepath.Join(delivery.SlotDirectory(archive+".slots", 1), "Payload.lua"))
			if err != nil || !bytes.Equal(retainedPayload, originalPayload) {
				t.Fatalf("original payload changed: %q %v", retainedPayload, err)
			}
			retainedMarker, err := os.ReadFile(filepath.Join(archive+".slots", ".lycheedev-slots.json"))
			if errors.Is(markerErr, os.ErrNotExist) {
				if !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("missing original manifest invented: %q %v", retainedMarker, err)
				}
			} else if err != nil || !bytes.Equal(retainedMarker, originalMarker) {
				t.Fatalf("original manifest changed: %q %v", retainedMarker, err)
			}
			if _, err := os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("slot-only repair replaced the main addon: %v", err)
			}
		})
	}
}

func TestDoctorRepairHardResetDoesNotBypassWindowClaim(t *testing.T) {
	p, report, parent := doctorRepairSlotFixture(t)
	ctx := context.Background()
	owner := journal.WindowOwner{WorkspaceID: strings.Repeat("a", 32), Resource: "window/11/22/33", OperationID: "CON-" + strings.Repeat("b", 32), IntentSHA256: strings.Repeat("c", 64)}
	if err := journal.BeginConnectionWindow(ctx, parent, owner, func() error { return nil }); err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(parent, ".lycheedev-slots.json")
	before, err := os.ReadFile(marker)
	if err != nil {
		t.Fatal(err)
	}
	archive := filepath.Join(t.TempDir(), "repair")
	if _, err := p.repairDoctor(ctx, DoctorRepairOptions{Archive: archive, HardReset: true}, report); !errors.Is(err, journal.ErrBusy) {
		t.Fatalf("reset bypassed claim: %v", err)
	}
	after, err := os.ReadFile(marker)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatalf("busy reset changed the pool: %v", err)
	}
	if _, err := os.Stat(archive + ".slots"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("busy reset created an archive: %v", err)
	}
}

func TestLiveDoctorUpgradePreservesExplicitArchive(t *testing.T) {
	parent := t.TempDir()
	testLiveDoctorArchiveUpgrade(t, testkit.Client(t, "flavor"), t.TempDir(), parent, filepath.Join(parent, "chosen-repair"), "", false)
}

func TestDoctorRepairUpgradesValidOlderSlotsWithoutReset(t *testing.T) {
	project := t.TempDir()
	testLiveDoctorArchiveUpgrade(t, testkit.Client(t, "flavor"), project, filepath.Join(project, ".lycheedev", "live", "repairs"), "", "slot-runtime", false)
}

func TestDoctorRepairHardResetAcceptsLegacyAddonWithoutSlots(t *testing.T) {
	project := t.TempDir()
	testLiveDoctorArchiveUpgrade(t, testkit.Client(t, "flavor"), project, filepath.Join(project, ".lycheedev", "live", "repairs"), "", "", true)
}

// This is a native disk-only test, not a game integration test. Distinct drive
// letters alone are insufficient: SUBST and junctions can alias one volume.
func TestLiveDoctorUpgradeDefaultArchiveAcrossVolumes(t *testing.T) {
	roots := liveDoctorWritableVolumeRoots(t)
	if len(roots) < 2 {
		t.Skipf("cross-volume doctor requires two existing writable Windows volumes with distinct GUIDs; discovered %d (see volume diagnostics)", len(roots))
	}
	run := func(t *testing.T, projectVolume, clientVolume string, sameVolume, explicitArchive bool) {
		t.Helper()
		project, err := os.MkdirTemp(projectVolume, "project-")
		if err != nil {
			t.Fatal(err)
		}
		clientRoot, err := os.MkdirTemp(clientVolume, "client-")
		if err != nil {
			t.Fatal(err)
		}
		client := filepath.Join(clientRoot, "_retail_")
		if err := os.CopyFS(client, os.DirFS(testkit.Client(t, "flavor"))); err != nil {
			t.Fatal(err)
		}
		projectGUID, projectErr := liveDoctorVolumeGUID(project)
		clientGUID, clientErr := liveDoctorVolumeGUID(client)
		if projectErr != nil || clientErr != nil || (projectGUID == clientGUID) != sameVolume {
			t.Fatalf("fixture volume mismatch: sameVolume=%t project=%q (%v) client=%q (%v)", sameVolume, projectGUID, projectErr, clientGUID, clientErr)
		}
		// Expectations come from the fixture's assigned volume case, not from the
		// production archive-selection logic or its filepath.VolumeName check.
		parent := filepath.Join(project, ".lycheedev", "live", "repairs")
		if !sameVolume {
			parent = filepath.Join(client, "Interface", ".lycheedev-repairs")
		}
		output := ""
		if explicitArchive {
			parent = filepath.Join(clientRoot, "chosen-archives")
			if err := os.Mkdir(parent, 0700); err != nil {
				t.Fatal(err)
			}
			output = filepath.Join(parent, "chosen-repair")
		}
		testLiveDoctorArchiveUpgrade(t, client, project, parent, output, "", false)
	}
	if !t.Run("same_volume_control", func(t *testing.T) { run(t, roots[0], roots[0], true, false) }) {
		t.Fatal("same-volume control failed; cross-volume result would not isolate the volume boundary")
	}
	t.Run("cross_volume_default_archive", func(t *testing.T) { run(t, roots[0], roots[1], false, false) })
	t.Run("cross_volume_explicit_archive", func(t *testing.T) { run(t, roots[0], roots[1], false, true) })
}

func doctorRepairRelease(t *testing.T, variant, version string) string {
	t.Helper()
	oldRelease := testkit.Release(t, variant)
	// Keep a complete, valid older release, including the TOC inventory hash.
	raw, err := os.ReadFile(filepath.Join(oldRelease, "release.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest delivery.Release
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	manifest.Version = version
	for i := range manifest.Resources {
		r := &manifest.Resources[i]
		if !strings.HasSuffix(r.Path, ".toc") {
			continue
		}
		path := filepath.Join(oldRelease, "payload", filepath.FromSlash(r.Path))
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		raw = []byte(strings.ReplaceAll(string(raw), "## Version: "+buildinfo.Version, "## Version: "+version))
		digest := sha256.Sum256(raw)
		r.Bytes, r.SHA256 = int64(len(raw)), hex.EncodeToString(digest[:])
		if err := os.WriteFile(path, raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
	raw, err = json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(oldRelease, "release.json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
	return oldRelease
}

func testLiveDoctorArchiveUpgrade(t *testing.T, client, projectDirectory, parent, output, variant string, hardReset bool) {
	t.Helper()
	ctx := context.Background()
	const oldVersion = "3.1.1"
	oldRelease := doctorRepairRelease(t, variant, oldVersion)
	if _, err := delivery.InstallAddon(ctx, oldRelease, client, oldVersion); err != nil {
		t.Fatal(err)
	}
	deployment, err := delivery.InspectAddonDeployment(ctx, client)
	if err != nil || deployment.Installation.State != "managed" {
		t.Fatalf("old deployment: %+v %v", deployment, err)
	}
	project, err := OpenProject(projectDirectory)
	if err != nil {
		t.Fatal(err)
	}
	if output == "" {
		if _, err := os.Stat(parent); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("fixture already contains repairs parent: %v", err)
		}
	}
	// A zero PID stops Activate before process discovery or input. The real
	// doctor repair branch must complete the disk upgrade before reaching it.
	report := DoctorResult{
		Target:     &live.ClientWindow{Client: deployment.Client},
		Deployment: &DoctorDeployment{State: "managed", Version: oldVersion, Slots: deployment.Slots},
	}
	projectGUID, projectVolumeErr := liveDoctorVolumeGUID(project.Root)
	clientGUID, clientVolumeErr := liveDoctorVolumeGUID(client)
	t.Logf("disk-only fixture: project=%q volume=%q (%v); client=%q volume=%q (%v); PID=%d",
		project.Root, projectGUID, projectVolumeErr, client, clientGUID, clientVolumeErr, report.Target.Window.ProcessID)
	rel, relErr := filepath.Rel(strings.ToLower(filepath.Dir(delivery.AddonDirectory(client))), strings.ToLower(filepath.Join(parent, "probe")))
	t.Logf("native archive/discovery Rel: relative=%q error=%v", rel, relErr)
	opts := DoctorRepairOptions{Release: testkit.Release(t, variant), Archive: output, HardReset: hardReset}
	_, repairErr := project.repairDoctor(ctx, opts, report)

	// Inspect before asserting the activation boundary so a native failure still
	// reports whether the original tree, staged replacement and archive survive.
	current, currentErr := delivery.InspectInstallation(ctx, delivery.AddonDirectory(client), "addon")
	originalJSON, _ := json.Marshal(deployment.Installation.Receipt)
	currentReceiptJSON, _ := json.Marshal(current.Receipt)
	t.Logf("original receipt: %s; original tree retained intact at target=%t", liveDoctorReceiptSummary(deployment.Installation.Receipt),
		currentErr == nil && current.State == "managed" && string(currentReceiptJSON) == string(originalJSON))
	t.Logf("doctor error (%T): %v; current state=%q receipt=(%s); inspect error=%v", repairErr, repairErr,
		current.State, liveDoctorReceiptSummary(current.Receipt), currentErr)
	archives, archiveErr := filepath.Glob(filepath.Join(parent, "*"))
	_, parentErr := os.Stat(parent)
	stage := "before recovery archive publication"
	if len(archives) > 0 {
		stage = "recovery archive published"
	}
	if errors.Is(repairErr, windows.ERROR_NOT_SAME_DEVICE) {
		stage = "cross-volume directory publication"
	}
	if repairErr != nil && repairErr.Error() == "live.channel_explicit_target_required" {
		stage = "safe activation boundary reached"
	}
	t.Logf("observed stage=%q; expected archive parent=%q error=%v; entries=%v; glob error=%v", stage, parent, parentErr, archives, archiveErr)
	for _, archive := range archives {
		raw, readErr := os.ReadFile(filepath.Join(archive, "replacement.json"))
		var journal struct {
			Schema, Target string
			Previous, Next *delivery.InstallationReceipt
		}
		decodeErr := json.Unmarshal(raw, &journal)
		t.Logf("replacement journal=%q schema=%q target=%q previous=(%s) next=(%s); read error=%v decode error=%v",
			archive, journal.Schema, journal.Target, liveDoctorReceiptSummary(journal.Previous), liveDoctorReceiptSummary(journal.Next), readErr, decodeErr)
		for _, tree := range []string{"previous", "next"} {
			state, err := delivery.InspectInstallation(ctx, filepath.Join(archive, tree), "addon")
			t.Logf("archive tree=%q state=%q receipt=(%s); inspect error=%v", filepath.Join(archive, tree),
				state.State, liveDoctorReceiptSummary(state.Receipt), err)
		}
	}
	if repairErr == nil || repairErr.Error() != "live.channel_explicit_target_required" {
		t.Errorf("repair failed before safe activation boundary: %v", repairErr)
	}
	if currentErr != nil || current.State != "managed" || current.Receipt == nil || current.Receipt.Version != buildinfo.Version {
		t.Errorf("upgrade did not publish: state=%q receipt=(%s) error=%v", current.State, liveDoctorReceiptSummary(current.Receipt), currentErr)
	}
	if archiveErr != nil || len(archives) != 1 {
		t.Fatalf("archive: %v %v", archives, archiveErr)
	}
	previous, err := delivery.InspectInstallation(ctx, filepath.Join(archives[0], "previous"), "addon")
	if err != nil || previous.State != "managed" || previous.Receipt == nil || previous.Receipt.Version != oldVersion {
		t.Fatalf("old installation not retained: %+v %v", previous, err)
	}
	previousJSON, _ := json.Marshal(previous.Receipt)
	if string(previousJSON) != string(originalJSON) {
		t.Fatalf("archived receipt changed: original=(%s) archived=(%s)",
			liveDoctorReceiptSummary(deployment.Installation.Receipt), liveDoctorReceiptSummary(previous.Receipt))
	}
	if output != "" {
		if archives[0] != output {
			t.Fatalf("explicit archive relocated: got %q want %q", archives[0], output)
		}
		journalPath := filepath.Join(output, "replacement.json")
		journalBefore, err := os.ReadFile(journalPath)
		if err != nil {
			t.Fatal(err)
		}
		// A repeated explicit destination must remain a conflict, preserving its
		// existing recovery record and old tree rather than choosing a new path.
		if _, err := project.repairDoctor(ctx, opts, report); !errors.Is(err, delivery.ErrConflict) {
			t.Fatalf("existing explicit archive was not preserved: %v", err)
		}
		journalAfter, err := os.ReadFile(journalPath)
		if err != nil || string(journalAfter) != string(journalBefore) {
			t.Fatalf("existing recovery journal changed: %v", err)
		}
		retained, err := delivery.InspectInstallation(ctx, filepath.Join(output, "previous"), "addon")
		retainedJSON, _ := json.Marshal(retained.Receipt)
		if err != nil || retained.State != "managed" || string(retainedJSON) != string(originalJSON) {
			t.Fatalf("existing archived tree changed: %+v %v", retained, err)
		}
		for _, unused := range []string{
			filepath.Join(project.Root, ".lycheedev", "live", "repairs"),
			filepath.Join(client, "Interface", ".lycheedev-repairs"),
		} {
			if _, err := os.Stat(unused); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("explicit archive created a default repair parent %q: %v", unused, err)
			}
		}
	}
}

func liveDoctorReceiptSummary(receipt *delivery.InstallationReceipt) string {
	if receipt == nil {
		return "absent"
	}
	raw, _ := json.Marshal(receipt)
	return fmt.Sprintf("version=%q SHA256=%x", receipt.Version, sha256.Sum256(raw))
}

// Only probe existing fixed drives and test-owned temporary directories. Never
// create/mount a volume, change TEMP, or use a real client installation.
func liveDoctorWritableVolumeRoots(t *testing.T) []string {
	t.Helper()
	candidates := []string{t.TempDir()}
	n, err := windows.GetLogicalDriveStrings(0, nil)
	if err != nil {
		t.Skipf("cannot discover existing Windows volumes: %v", err)
	}
	buffer := make([]uint16, n+1)
	n, err = windows.GetLogicalDriveStrings(uint32(len(buffer)), &buffer[0])
	if err != nil || n >= uint32(len(buffer)) {
		t.Skipf("cannot read existing Windows volumes: length=%d capacity=%d error=%v", n, len(buffer), err)
	}
	for start := 0; start < int(n); {
		drive := windows.UTF16ToString(buffer[start:])
		if drive == "" {
			break
		}
		start += len(drive) + 1 // Drive roots contain only ASCII drive letters.
		root, err := windows.UTF16PtrFromString(drive)
		if err != nil {
			t.Fatal(err)
		}
		if kind := windows.GetDriveType(root); kind != windows.DRIVE_FIXED {
			t.Logf("volume candidate %q ignored: drive type %d is not a fixed local drive", drive, kind)
			continue
		}
		candidates = append(candidates, drive)
	}
	seen := map[string]bool{}
	var roots []string
	for _, candidate := range candidates {
		guid, err := liveDoctorVolumeGUID(candidate)
		if err != nil {
			t.Logf("volume candidate %q ignored: no verified volume GUID: %v", candidate, err)
			continue
		}
		if seen[guid] {
			t.Logf("volume candidate %q aliases already selected volume %q", candidate, guid)
			continue
		}
		root, err := os.MkdirTemp(candidate, "lycheedev-doctor-")
		if err != nil {
			t.Logf("volume candidate %q (%s) not writable: %v", candidate, guid, err)
			continue
		}
		t.Cleanup(func() {
			if err := os.RemoveAll(root); err != nil {
				t.Errorf("remove test-owned volume directory %q: %v", root, err)
			}
		})
		actual, err := liveDoctorVolumeGUID(root)
		if err != nil || actual != guid {
			t.Fatalf("temporary directory volume identity changed: %q want=%q actual=%q error=%v", root, guid, actual, err)
		}
		t.Logf("writable native volume: root=%q GUID=%q", root, guid)
		seen[guid] = true
		roots = append(roots, root)
	}
	return roots
}

func liveDoctorVolumeGUID(directory string) (string, error) {
	file, err := os.Open(directory)
	if err != nil {
		return "", err
	}
	defer file.Close()
	// GetFinalPathNameByHandle resolves the actual opened directory, including
	// SUBST/junction aliases. VOLUME_NAME_GUID requests its native volume name.
	const volumeNameGUID = 0x1
	buffer := make([]uint16, 32768)
	n, err := windows.GetFinalPathNameByHandle(windows.Handle(file.Fd()), &buffer[0], uint32(len(buffer)), volumeNameGUID)
	if err != nil {
		return "", err
	}
	if n >= uint32(len(buffer)) {
		return "", fmt.Errorf("resolved volume path exceeds %d UTF-16 code units", len(buffer))
	}
	path := windows.UTF16ToString(buffer[:n])
	end := strings.Index(path, `}\`)
	if !strings.HasPrefix(strings.ToLower(path), `\\?\volume{`) || end < 0 {
		return "", fmt.Errorf("expected native volume GUID path, got %q", path)
	}
	return strings.ToLower(path[:end+2]), nil
}
