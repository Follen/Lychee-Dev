//go:build windows && amd64 && lycheedev_channel_lab

// Native acceptance harness for the new production components. It is excluded
// from release builds; the ordinary command surface is wired after these gates.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"sync"
	"time"

	"golang.org/x/sys/windows"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"github.com/follenfang/lycheedev/internal/vault"
)

func save(ctx context.Context, path string, value any) error {
	b, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, path, b)
}
func run() (resultErr error) {
	mode := flag.String("mode", "discover", "discover, capture, install, reload, connect, run, resume, disconnect")
	client := flag.String("installation", "", "explicit installation")
	pid := flag.Uint("pid", 0, "exact process id")
	project := flag.String("project", "", "acceptance project directory")
	code := flag.String("file", "", "probe file")
	release := flag.String("release", "", "candidate release root")
	cache := flag.Bool("cache", true, "disposable hints")
	reloadHint := flag.Bool("reload-hint", true, "capture optional reload color hint; false leaves runtime verification to public resume")
	seconds := flag.Int("timeout", 120, "bounded wall time")
	budget := flag.Int("budget", 15, "probe seconds")
	connection := flag.String("connection", "", "project CON id for controlled observation reload fault")
	flag.Parse()
	if *client == "" || *pid == 0 || *project == "" || *seconds < 1 || *seconds > 300 {
		return errors.New("explicit installation, pid, project and bounded timeout required")
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(*seconds)*time.Second)
	defer cancel()
	target, err := live.ResolveClientWindow(ctx, *client, uint32(*pid))
	if err != nil {
		return err
	}
	root, err := filepath.Abs(*project)
	if err != nil {
		return err
	}
	stateRoot := filepath.Join(root, ".lycheedev", "live")
	parent := filepath.Join(target.Client.Directory, "Interface", "AddOns")
	resource := fmt.Sprintf("window/%d/%d/%d", target.Window.ProcessID, target.Window.ProcessStartedAt, target.Window.Handle)
	stamp := time.Now().UTC().Format("20060102T150405.000000000")
	evidence := filepath.Join(root, "evidence", stamp+"-"+*mode)
	if err = os.MkdirAll(evidence, 0700); err != nil {
		return err
	}
	if err = save(ctx, filepath.Join(evidence, "target.json"), target); err != nil {
		return err
	}
	assessment, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(*client), "addon")
	if err != nil {
		return err
	}
	if *mode == "install" {
		if *release == "" {
			return errors.New("release required")
		}
		archive := filepath.Join(root, "install-transaction-"+stamp)
		if assessment.State == "absent" {
			_, err = delivery.InstallAddon(ctx, *release, *client, buildinfo.Version)
		} else {
			_, err = delivery.UpgradeAddon(ctx, *release, *client, archive, buildinfo.Version, false)
		}
		return err
	}
	guardBase := func(ctx context.Context) error {
		if err := live.ConfirmClientWindow(ctx, target); err != nil {
			return err
		}
		now, err := delivery.InspectInstallation(ctx, delivery.AddonDirectory(*client), "addon")
		if err != nil {
			return err
		}
		if now.State != "managed" || now.Receipt == nil || now.Receipt.Version != buildinfo.Version {
			return errors.New("clean exact-version addon required")
		}
		return nil
	}
	capture := func(name string) error {
		stream, err := desktop.CaptureFrames(ctx, target.Window, desktop.WholeWindowCapture())
		if err != nil {
			return err
		}
		defer stream.Close()
		frame, err := stream.Next(ctx)
		if err != nil {
			return err
		}
		f, err := os.Create(filepath.Join(evidence, name+".png"))
		if err != nil {
			return err
		}
		return errors.Join(png.Encode(f, frame.NRGBA), f.Close())
	}
	if *mode == "capture" {
		return capture("frame")
	}
	if *mode == "input-signal" {
		stream, err := desktop.CaptureFramesWithStartupTimeout(ctx, target.Window, desktop.InputSignalCapture(), 2*time.Second)
		if err != nil {
			return err
		}
		defer stream.Close()
		type sample struct {
			Ticks  int64              `json:"ticks"`
			AgeMS  int64              `json:"ageMs"`
			Signal bridge.InputSignal `json:"signal"`
			Error  string             `json:"error,omitempty"`
		}
		var samples []sample
		for len(samples) < 40 {
			frame, err := stream.Next(ctx)
			if err != nil {
				return err
			}
			now, err := desktop.CaptureSystemTicks()
			if err != nil {
				return err
			}
			age, err := desktop.FrameAge(frame, now)
			if err != nil {
				return err
			}
			signal, decodeErr := bridge.DecodeInputSignal(frame.NRGBA)
			s := sample{Ticks: frame.SystemTicks, AgeMS: age.Milliseconds(), Signal: signal}
			if decodeErr != nil {
				s.Error = decodeErr.Error()
			}
			samples = append(samples, s)
		}
		return save(ctx, filepath.Join(evidence, "input-signal.json"), struct {
			Window  desktop.WindowIdentity `json:"window"`
			Area    any                    `json:"area"`
			Samples []sample               `json:"samples"`
		}{target.Window, stream.CaptureArea(), samples})
	}
	reload := func() error {
		if err = guardBase(ctx); err != nil {
			return err
		}
		if err = capture("before"); err != nil {
			return err
		}
		receipt, inputErr := desktop.WithReceiverInput(ctx, target.Window, guardBase, func(in *desktop.ReceiverInput) error {
			return in.FixedReload(func(step int) error {
				return save(ctx, filepath.Join(evidence, fmt.Sprintf("input-%d.json", step)), map[string]any{"attempted": step, "window": target.Window})
			})
		})
		if e := save(ctx, filepath.Join(evidence, "reload-input.json"), map[string]any{"receipt": receipt, "identityVerified": false}); e != nil {
			return e
		}
		if inputErr != nil {
			return inputErr
		}
		if !*reloadHint {
			return nil
		}
		stream, err := desktop.CaptureFrames(ctx, target.Window, desktop.WholeWindowCapture())
		if err != nil {
			return err
		}
		defer stream.Close()
		previous, ticks, transitions := -1, int64(0), 0
		for frames := 0; frames < 1200; frames++ {
			frame, err := stream.Next(ctx)
			if err != nil {
				return err
			}
			if frame.SystemTicks <= ticks {
				continue
			}
			ticks = frame.SystemTicks
			patch, found := desktop.ReadReloadPatch(frame.NRGBA)
			if !found {
				continue
			}
			if previous >= 0 && patch.Phase == (previous+1)%3 {
				transitions++
			}
			previous = patch.Phase
			if transitions >= 2 {
				f, err := os.Create(filepath.Join(evidence, "reload-patch.png"))
				if err != nil {
					return err
				}
				err = errors.Join(png.Encode(f, frame.NRGBA), f.Close())
				if err != nil {
					return err
				}
				return save(ctx, filepath.Join(evidence, "reload-hint.json"), map[string]any{"transitions": transitions, "ticks": ticks, "identityVerified": false})
			}
		}
		return errors.New("reload patch not observed; input result remains unconfirmed")
	}
	if *mode == "stage-observation" {
		return stageObservation(ctx, root, parent, resource, *connection, evidence, target, guardBase)
	}
	if *mode == "hold-publication" || *mode == "interrupt-observation" || *mode == "interrupt-opaque-fixture" || *mode == "release-editor" || *mode == "confirm-focus-fixture" {
		projectAPI, e := channel.OpenProject(root)
		if e != nil {
			return e
		}
		if _, e = projectAPI.Status(*connection); e != nil {
			return e
		}
		var meta struct {
			Target live.ClientWindow
			Owner  journal.WindowOwner
		}
		b, e := os.ReadFile(filepath.Join(stateRoot, "connections", *connection+".target.json"))
		if e != nil {
			return e
		}
		if e = json.Unmarshal(b, &meta); e != nil {
			return e
		}
		if meta.Target != target {
			return errors.New("fault injection target mismatch")
		}
		if *mode == "hold-publication" {
			if e = guardBase(ctx); e != nil {
				return e
			}
			owner, busy, e := journal.InspectWindowOwner(ctx, parent, resource)
			if e != nil {
				return e
			}
			if !busy || owner != meta.Owner {
				return errors.New("fault injection ownership changed")
			}
			// Fault injection owns only the shared publication lease. It sends no
			// input and never changes slots or connection journals.
			held, e := vault.AcquireLease(ctx, filepath.Join(parent, ".lycheedev-slot-locks"), "publication")
			if e != nil {
				return e
			}
			defer held.Close()
			if e = save(ctx, filepath.Join(evidence, "held.json"), map[string]any{"inputSent": false, "connection": *connection}); e != nil {
				return e
			}
			fmt.Println(`{"publicationHeld":true,"inputSent":false}`)
			<-ctx.Done()
			return held.Close()
		}
		lease, e := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
		if e != nil {
			return e
		}
		defer lease.Close()
		d, e := channel.Load(filepath.Join(stateRoot, "connections", *connection+".jsonl"), nil)
		if e != nil {
			return e
		}
		policy := "observation"
		if *mode == "interrupt-opaque-fixture" {
			policy = "opaque"
		}
		if *mode == "confirm-focus-fixture" {
			if !d.State.Bound || d.State.Operation == nil || d.State.Operation.Request != "focus-singleline" || d.State.Operation.Policy != "observation" || d.State.Operation.Stage != "confirm_ready" || d.State.Transaction == nil || d.State.Transaction.Envelope.Action != "confirm" {
				return errors.New("focus recovery requires the exact pending single-line fixture confirmation")
			}
			fixture, err := os.ReadFile(filepath.Join(*project, "focus-singleline.lua"))
			if err != nil || string(fixture) != d.State.Operation.Code {
				return errors.New("focus fixture bytes changed")
			}
		} else if *mode == "release-editor" {
			if !d.State.Bound || d.State.Operation == nil || (d.State.Operation.Request != "runner" && d.State.Operation.Request != "objects" && d.State.Operation.Request != "trace") || d.State.Operation.Policy != "observation" || d.State.Operation.Stage != "confirm_ready" || d.State.Transaction == nil || d.State.Transaction.Envelope.Action != "confirm" {
				return errors.New("editor recovery requires the pending visual runner observation")
			}
		} else if *mode == "interrupt-observation" && d.State.Bound && d.State.Operation != nil && d.State.Operation.Policy == "observation" && d.State.Operation.Stage == "prepared" && d.State.Transaction != nil && d.State.Transaction.Envelope.Action == "prepare" {
			// Fault-inject reload without replaying an uncertain prepare key.
			// Recovery must discover a new runtime and retain the observation ID.
		} else if !d.State.Bound || d.State.Transaction != nil || d.State.Operation == nil || d.State.Operation.Stage != "running" || d.State.Operation.Policy != policy {
			return errors.New("fault injection requires an idle driver and running observation")
		}
		if policy == "opaque" {
			fixture, err := os.ReadFile("tests/channel-live/fixtures/async_reload.lua")
			if err != nil || string(fixture) != d.State.Operation.Code {
				return errors.New("opaque interruption is restricted to the checked-in read-only async fixture")
			}
		}
		if e = save(ctx, filepath.Join(evidence, "interrupted-state.json"), d.State); e != nil {
			return e
		}
		base := guardBase
		guardBase = func(ctx context.Context) error {
			if err := base(ctx); err != nil {
				return err
			}
			owner, busy, err := journal.InspectWindowOwner(ctx, parent, resource)
			if err != nil {
				return err
			}
			if !busy || owner != meta.Owner {
				return errors.New("fault injection ownership changed")
			}
			return nil
		}
		if *mode == "confirm-focus-fixture" {
			if err := save(ctx, filepath.Join(evidence, "confirm-wake-attempt.json"), d.State.Transaction); err != nil {
				return err
			}
			_, err := desktop.WithReceiverInputProfile(ctx, target.Window, desktop.SlotReceiverBindings(), guardBase, func(in *desktop.ReceiverInput) error { return in.Wake() })
			return err
		}
		if *mode == "release-editor" {
			// Stop the fixed sequence after exactly one Escape. It clears the
			// editor focus without chat input, reload or business replay.
			stop := errors.New("editor escape complete")
			_, err := desktop.WithReceiverInput(ctx, target.Window, guardBase, func(in *desktop.ReceiverInput) error {
				return in.FixedReload(func(step int) error {
					if step != 1 {
						return stop
					}
					return save(ctx, filepath.Join(evidence, "escape-attempt.json"), map[string]any{"connection": *connection, "window": target.Window})
				})
			})
			if errors.Is(err, stop) {
				// This is the exact read-only confirmation slot, never prepare or
				// commit. Retain the additional manual recovery attempt separately.
				if err = save(ctx, filepath.Join(evidence, "confirm-wake-attempt.json"), d.State.Transaction); err != nil {
					return err
				}
				_, err = desktop.WithReceiverInput(ctx, target.Window, guardBase, func(in *desktop.ReceiverInput) error { return in.Wake() })
				return err
			}
			return err
		}
		return reload()
	}
	if *mode == "reload" {
		gate, e := journal.AcquireInstallationMaintenance(ctx, parent)
		if e != nil {
			return e
		}
		defer gate.Close()
		return reload()
	}
	native, err := channel.OpenNative(target.Window, parent, buildinfo.Version, filepath.Join(stateRoot, "cache", "hints.json"), *cache)
	if err != nil {
		return err
	}
	defer func() { resultErr = errors.Join(resultErr, native.Close()) }()
	defer func() { _ = save(context.Background(), filepath.Join(evidence, "scans.json"), native.Lookups) }()
	if *mode == "scan-benchmark" {
		for _, workers := range []int{1, 8} {
			result, e := memory.Scan(ctx, native.Process, [][]byte{bridge.MemoryMagic}, memory.Options{Workers: workers})
			if err = save(context.Background(), filepath.Join(evidence, fmt.Sprintf("workers-%d.json", workers)), result.Coverage); err != nil {
				return err
			}
			if e != nil {
				return e
			}
			if err = json.NewEncoder(os.Stdout).Encode(result.Coverage); err != nil {
				return err
			}
		}
		return nil
	}
	if *mode == "input-state" {
		var lock sync.Mutex
		minimumAge := int64(1 << 62)
		tick := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetTickCount64")
		selector := memory.Selector{Kind: bridge.MemoryInputState, Accept: func(r memory.Record) bool {
			var state channel.InputObservation
			if json.Unmarshal(r.Payload, &state) != nil {
				return false
			}
			now, _, _ := tick.Call()
			lock.Lock()
			if age := int64(now) - state.SampleMillis; age < minimumAge {
				minimumAge = age
			}
			lock.Unlock()
			return true
		}}
		found, findErr := native.Find(ctx, selector, false)
		var latest channel.InputObservation
		for _, record := range found.Records {
			var state channel.InputObservation
			if json.Unmarshal(record.Payload, &state) == nil && state.SampleMillis > latest.SampleMillis {
				latest = state
			}
		}
		if err = json.NewEncoder(os.Stdout).Encode(map[string]any{"latest": latest, "minimumAgeMillis": minimumAge, "coverage": found.Coverage}); err != nil {
			return err
		}
		return findErr
	}
	if *mode == "discover" {
		identities, coverage, err := native.Discover(ctx, "", "")
		if err != nil {
			return err
		}
		value := map[string]any{"candidates": identities, "coverage": coverage}
		if err = save(ctx, filepath.Join(evidence, "discovery.json"), value); err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(value)
	}
	log := filepath.Join(stateRoot, "connections", "acceptance.jsonl")
	metaPath := filepath.Join(stateRoot, "connections", "target.json")
	var owner journal.WindowOwner
	var d *channel.Driver
	if *mode == "connect" {
		if _, err = os.Stat(log); !errors.Is(err, os.ErrNotExist) {
			return errors.New("existing journal: resume or use another acceptance project")
		}
		identities, _, err := native.Discover(ctx, "", "")
		if err != nil {
			return err
		}
		if len(identities) == 0 {
			return errors.New("no memory-slot runtime discovered")
		}
		d, err = channel.New(log, native, identities[0])
		if err != nil {
			return err
		}
		projectHash := sha256.Sum256([]byte(root))
		intentHash := sha256.Sum256([]byte(resource + "/" + d.State.ID))
		owner = journal.WindowOwner{Schema: "lycheedev.window-owner.v1", WorkspaceID: fmt.Sprintf("%x", projectHash[:16]), Resource: resource, OperationID: d.State.ID, IntentSHA256: fmt.Sprintf("%x", intentHash)}
		err = journal.BeginConnectionWindow(ctx, parent, owner, func() error {
			if err := save(ctx, metaPath, map[string]any{"target": target, "owner": owner}); err != nil {
				return err
			}
			return d.Save(ctx, "connection_created")
		})
		if err != nil {
			return err
		}
	} else {
		b, err := os.ReadFile(metaPath)
		if err != nil {
			return err
		}
		var meta struct {
			Target live.ClientWindow
			Owner  journal.WindowOwner
		}
		if err = json.Unmarshal(b, &meta); err != nil {
			return err
		}
		if meta.Target != target {
			return errors.New("saved target changed")
		}
		owner = meta.Owner
		d, err = channel.Load(log, native)
		if err != nil {
			return err
		}
	}
	lease, err := journal.LockBootstrapWindow(ctx, parent, owner)
	if err != nil {
		return err
	}
	defer func() {
		if lease != nil {
			_ = lease.Close()
		}
	}()
	native.Guard = func(ctx context.Context) error {
		if err := guardBase(ctx); err != nil {
			return err
		}
		current, busy, err := journal.InspectWindowOwner(ctx, parent, resource)
		if err != nil {
			return err
		}
		if !busy || current != owner {
			return errors.New("connection ownership changed")
		}
		return nil
	}
	if err = native.Guard(ctx); err != nil {
		return err
	}
	switch *mode {
	case "connect":
		err = d.Connect(ctx)
	case "reload-bootstrap", "retry-bootstrap", "recover-bootstrap":
		if d.State.Bound || d.State.Operation != nil || d.State.Transaction == nil || d.State.Transaction.Envelope.Action != "bind" {
			return errors.New("requires an unresolved first binding with no business operation")
		}
		if err = d.Save(ctx, *mode+"_intent"); err != nil {
			return err
		}
		if *mode == "reload-bootstrap" {
			err = reload()
		}
		if *mode == "retry-bootstrap" {
			err = native.RetryFirstBinding(ctx, d.State.Transaction.Envelope, d.State.Identity.InputState)
		}
		if *mode == "recover-bootstrap" {
			err = d.RecoverBinding(ctx)
		}
	case "repair-bootstrap":
		if d.State.Bound || d.State.Operation != nil || d.State.Transaction == nil || d.State.Transaction.Envelope.Action != "bind" {
			return errors.New("repair requires an unresolved first binding with no business operation")
		}
		publication, e := vault.TryAcquireLease(ctx, filepath.Join(parent, ".lycheedev-slot-locks"), "publication")
		if e != nil {
			return e
		}
		defer publication.Close()
		if err = d.Save(ctx, "bootstrap_template_repair_intent"); err != nil {
			return err
		}
		err = delivery.RepairSlotBootstrap(ctx, parent, buildinfo.Version)
		if err == nil {
			err = d.Save(ctx, "bootstrap_template_repaired")
		}
	case "run":
		b, e := os.ReadFile(*code)
		if e != nil {
			return e
		}
		if err = d.PrepareOperation(ctx, string(b), *budget, "observation"); err != nil {
			return err
		}
		err = d.Continue(ctx)
	case "resume":
		err = d.Continue(ctx)
	case "disconnect":
		err = d.Disconnect(ctx)
	default:
		return errors.New("unknown mode")
	}
	if saveErr := save(context.Background(), filepath.Join(evidence, "state.json"), d.State); err == nil {
		err = saveErr
	}
	if err == nil && *mode == "disconnect" {
		if err = lease.Close(); err == nil {
			lease = nil
			err = journal.RetireConnectionWindow(ctx, parent, owner)
		}
	}
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(d.State)
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
