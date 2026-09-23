package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Cancel ends only an operation that has not prepared a queue or submitted
// game input. Later stages retain their ownership and must be resumed through
// verified cleanup; a missing effect receipt can never authorize cancellation.
func Cancel(ctx context.Context, root, id string) (Outcome, error) {
	record, err := withOperation(ctx, root, id, func(store *vault.Store, metadata *vault.Metadata) (journal.WorkRecord, error) {
		book := journal.OpenBook(metadata)
		record, err := book.InspectWork(ctx, id)
		if err != nil {
			return record, err
		}
		if record.Intent.Kind != "probe" {
			return record, journal.ErrTransition
		}
		input, _, err := probeDefinition(record)
		if err != nil {
			return record, err
		}
		parent := filepath.Join(input.Load.Installation, "Interface", "AddOns")
		if record.Stage == "cleaned" && record.Status == "cancelled" {
			return record, book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
		}
		if record.Stage != "prepared" || record.Status != "pending" || len(record.Observation) != 0 && string(record.Observation) != "null" {
			return record, journal.ErrTransition
		}
		run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
		if err != nil {
			return record, err
		}
		if _, err := run.Check(ctx); err != nil {
			return record, errors.Join(err, run.Close())
		}
		err = book.AdvanceStage(ctx, journal.StageChange{OperationID: id, ExpectedGeneration: record.Generation, ExpectedStage: "prepared", Stage: "cleaned", Status: "cancelled", Observation: json.RawMessage(`null`)})
		closeErr := run.Close()
		if err != nil || closeErr != nil {
			return record, errors.Join(err, closeErr)
		}
		record, err = book.InspectWork(ctx, id)
		if err != nil {
			return record, err
		}
		return record, book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
	})
	return finishOutcome(ctx, root, record, err)
}
