package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Legacy operations can be inspected or explicitly retired, never resumed.
// The original intent and observation remain intact, including uncertain input.
func Abandon(ctx context.Context, root, id string) (Outcome, error) {
	return retireLegacyOperation(ctx, root, id, false)
}

// Cancel applies only before any recorded effect. All other legacy work needs
// explicit abandonment; neither action claims a game-side acknowledgement.
func Cancel(ctx context.Context, root, id string) (Outcome, error) {
	return retireLegacyOperation(ctx, root, id, true)
}

func legacyInstallation(record journal.WorkRecord) (string, error) {
	var input struct {
		Installation string           `json:"installation"`
		Load         *ProbeLoadIntent `json:"load"`
		Target       ClientWindow     `json:"target"`
	}
	if err := json.Unmarshal(record.Intent.Request, &input); err != nil {
		return "", err
	}
	directory := input.Installation
	if input.Load != nil {
		directory = input.Load.Installation
	}
	if input.Target.Client.Directory != "" {
		directory = input.Target.Client.Directory
	}
	if !filepath.IsAbs(directory) {
		return "", errors.New("live.legacy_installation_missing")
	}
	return filepath.Join(directory, "Interface", "AddOns"), nil
}

func retireLegacyOperation(ctx context.Context, root, id string, cancelOnly bool) (Outcome, error) {
	record, err := withOperation(ctx, root, id, func(store *vault.Store, metadata *vault.Metadata) (record journal.WorkRecord, err error) {
		book := journal.OpenBook(metadata)
		record, err = book.InspectWork(ctx, id)
		if err != nil {
			return record, err
		}
		if record.Intent.Kind != "probe" && record.Intent.Kind != "faults" && record.Intent.Kind != "reload" {
			return record, journal.ErrTransition
		}
		parent, err := legacyInstallation(record)
		if err != nil {
			return record, err
		}
		if record.Stage == "abandoned" || record.Stage == "cleaned" {
			return record, book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
		}
		run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
		if err != nil {
			return record, err
		}
		defer func() { err = errors.Join(err, run.Close()) }()
		record, err = run.Check(ctx)
		if err != nil {
			return record, err
		}
		unstarted := record.Stage == "prepared" && record.Status == "pending" && (len(record.Observation) == 0 || string(record.Observation) == "null")
		if cancelOnly && !unstarted {
			return record, journal.ErrTransition
		}
		advance := func(stage, status string) error {
			observation := record.Observation
			if len(observation) == 0 {
				observation = json.RawMessage(`null`)
			}
			if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: id, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: observation}); err != nil {
				return err
			}
			var err error
			record, err = book.InspectWork(ctx, id)
			return err
		}
		if unstarted && cancelOnly {
			err = advance("cleaned", "cancelled")
		} else {
			if record.Stage != "abandoning" {
				err = advance("abandoning", "running")
			}
			if err == nil {
				err = advance("abandoned", "abandoned")
			}
		}
		if err != nil {
			return record, err
		}
		if err = run.Close(); err != nil {
			return record, err
		}
		return record, book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
	})
	if record.OperationID == "" {
		return Outcome{}, err
	}
	result, readErr := Status(ctx, root, id)
	if result.OperationID == "" {
		result = pendingOutcome(record)
	}
	return result, errors.Join(err, readErr)
}
