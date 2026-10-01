//go:build windows && amd64 && lycheedev_channel_lab

package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/channel"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

var errStageObservation = errors.New("lab.stage_observation_paused_before_input")

type stageBackend struct{ *channel.Native }

func (b *stageBackend) ObserveInput(ctx context.Context, _ bridge.SlotEnvelope, _ int64, _ string) (channel.InputObservation, error) {
	if err := b.Guard(ctx); err != nil {
		return channel.InputObservation{}, err
	}
	return channel.InputObservation{}, errStageObservation
}
func stageObservation(ctx context.Context, root, parent, resource, connection, evidence string, target live.ClientWindow, guardBase func(context.Context) error) (resultErr error) {
	api, err := channel.OpenProject(root)
	if err != nil {
		return err
	}
	if _, err = api.Status(connection); err != nil {
		return err
	}
	base := filepath.Join(root, ".lycheedev", "live")
	var meta struct {
		Target live.ClientWindow
		Owner  journal.WindowOwner
	}
	raw, err := os.ReadFile(filepath.Join(base, "connections", connection+".target.json"))
	if err != nil {
		return err
	}
	if err = json.Unmarshal(raw, &meta); err != nil {
		return err
	}
	if meta.Target != target {
		return errors.New("stage target mismatch")
	}
	lease, err := journal.LockBootstrapWindow(ctx, parent, meta.Owner)
	if err != nil {
		return err
	}
	defer lease.Close()
	native, err := channel.OpenNative(ctx, target.Window, parent, buildinfo.Version, filepath.Join(base, "cache", "hints.json"), true)
	if err != nil {
		return err
	}
	defer func() { resultErr = errors.Join(resultErr, native.Close()) }()
	native.Guard = func(ctx context.Context) error {
		if err := guardBase(ctx); err != nil {
			return err
		}
		owner, busy, err := journal.InspectWindowOwner(ctx, parent, resource)
		if err != nil {
			return err
		}
		if !busy || owner != meta.Owner {
			return errors.New("stage ownership changed")
		}
		return nil
	}
	if err = native.Guard(ctx); err != nil {
		return err
	}
	d, err := channel.Load(filepath.Join(base, "connections", connection+".jsonl"), &stageBackend{native})
	if err != nil {
		return err
	}
	if !d.State.Bound || d.State.Closed || d.State.Closing || d.State.RuntimeEnd != nil || d.State.Transaction != nil || d.State.Operation != nil || d.State.Reload != nil && d.State.Reload.Phase != "complete" || d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		return errors.New("stage requires fresh idle bound CON without business")
	}
	if d.State.Identity.Inventory == nil || *d.State.Identity.Inventory != 200 {
		return errors.New("stage requires 200-slot runtime")
	}
	before, _ := json.Marshal(d.State.Input)
	fixture, err := os.ReadFile("tests/channel-live/fixtures/slot_skip.lua")
	if err != nil {
		return err
	}
	if err = d.PrepareRequest(ctx, "slot-skip-a", string(fixture), 5, "observation"); err != nil {
		return err
	}
	err = d.Continue(ctx)
	if !errors.Is(err, errStageObservation) {
		return fmt.Errorf("stage did not pause at input gate: %w", err)
	}
	tx := d.State.Transaction
	after, _ := json.Marshal(d.State.Input)
	if tx == nil || tx.Phase != "published" || tx.Envelope.Action != "prepare" || string(before) != string(after) {
		return errors.New("stage failed published/no-input invariant")
	}
	if err = native.Guard(ctx); err != nil {
		return err
	}
	output := map[string]any{"staged": true, "inputSent": false, "connection": connection, "request": "slot-skip-a", "slot": tx.Envelope.Index, "nonce": tx.Envelope.Nonce, "operation": d.State.Operation.ID, "evidence": evidence}
	if err = save(ctx, filepath.Join(evidence, "staged.json"), output); err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(output)
}
