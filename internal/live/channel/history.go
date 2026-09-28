package channel

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live/journal"
)

// Rotate only at a quiescent boundary. Each immutable segment preserves its
// whole hash chain. Current operations retain enough space for bounded recovery
// and cleanup; historical requests still resolve to their original results.
func (d *Driver) checkpoint(ctx context.Context) error {
	if d.State.Transaction != nil || d.State.Reload != nil && d.State.Reload.Phase != "complete" || d.State.Recovery != nil && d.State.Recovery.Phase != "complete" {
		return ErrPending
	}
	if d.State.Operation != nil && d.State.Operation.Stage != "complete" && d.State.Operation.Stage != "execution_unknown" {
		return ErrPending
	}
	info, err := os.Stat(d.Log)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	events, err := journal.ReadMemoryLog(d.Log)
	if err != nil {
		return err
	}
	if len(events) < 256 && info.Size() < 8<<20 {
		return nil
	}
	previous := d.State.Archive
	d.State.Archive = events[len(events)-1].Checksum
	value, err := snapshot(ctx, d.Log, d.State)
	if err == nil {
		err = journal.CheckpointMemoryLog(ctx, d.Log, d.State.Archive, value)
	}
	if err != nil {
		d.State.Archive = previous
	}
	return err
}

// Load one bounded segment at a time. A missing/corrupt older segment is an
// error, never permission to treat an old request as new and execute it again.
func historicalState(path string, matches func(State) bool) (*State, error) {
	current, expected := path, ""
	seen := map[string]bool{}
	for segment := 0; segment < 1024; segment++ {
		events, err := journal.ReadMemoryLog(current)
		if err != nil {
			return nil, err
		}
		if len(events) == 0 {
			if expected != "" {
				return nil, ErrJournalMissing
			}
			return nil, nil
		}
		if expected != "" && events[len(events)-1].Checksum != expected {
			return nil, errors.New("live.channel_history_corrupt")
		}
		var latest State
		for i := len(events) - 1; i >= 0; i-- {
			var s State
			if err = json.Unmarshal(events[i].Data, &s); err != nil {
				return nil, err
			}
			if err = hydrate(path, &s); err != nil {
				return nil, err
			}
			if i == len(events)-1 {
				latest = s
			}
			if matches(s) {
				return &s, nil
			}
		}
		if latest.Archive == "" {
			return nil, nil
		}
		if seen[latest.Archive] {
			return nil, errors.New("live.channel_history_corrupt")
		}
		seen[latest.Archive] = true
		expected = latest.Archive
		current = filepath.Join(path+".history", expected+".jsonl")
	}
	return nil, errors.New("live.channel_history_limit")
}
