package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
)

// Abandon explicitly stops host-side cleanup without claiming a game ACK.
func Abandon(ctx context.Context, root, id string) (Outcome, error) {
	record, err := abandonProbe(ctx, root, id)
	return finishOutcome(ctx, root, record, err)
}

func abandonProbe(ctx context.Context, root, id string) (record journal.WorkRecord, err error) {
	store, err := vault.OpenStore(root)
	if err != nil {
		return record, err
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		return record, err
	}
	defer func() { err = errors.Join(err, metadata.Close()) }()
	book := journal.OpenBook(metadata)
	record, err = book.InspectWork(ctx, id)
	if err != nil {
		return record, err
	}
	if record.Intent.Kind != "probe" {
		return record, journal.ErrTransition
	}
	// A dispatched probe whose reported receipt was lost (hidden card, client
	// restart) wedges the window: every other exit requires the evidence that
	// no longer exists. Release is honest here only when the durable
	// observation proves the run input was fully queued; the in-game effect
	// stays unknown and a client reload clears the runtime queue copy.
	//
	// flush_requested is the same wedge one phase later: the host sent the
	// correlation reload, the report never reached SavedVariables, so neither
	// reconciliation nor acknowledgement can ever succeed. A stale reentry
	// ticket in the client's SavedVariables proves only that a reload was
	// requested, never that a report was stored, so it stays out of the proof.
	if record.Stage != "dispatch_requested" && record.Stage != "flush_requested" &&
		record.Stage != "verified" && record.Stage != "abandoning" && record.Stage != "abandoned" {
		return record, journal.ErrTransition
	}
	input, definition, err := probeDefinition(record)
	if err != nil {
		return record, err
	}
	parent := filepath.Join(input.Load.Installation, "Interface", "AddOns")
	verify := func() error {
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return err
		}
		if observed.AckInput != nil || observed.AcknowledgementID != "" {
			return journal.ErrTransition
		}
		if observed.Schema == "lycheedev.probe-load.v1" {
			if record.Stage == "verified" || observed.BodyID != "" || observed.ReceiptID != "" ||
				observed.DispatchInput == nil || observed.DispatchInput.MessagesQueued <= 0 ||
				!observed.DispatchInput.SubmissionComplete {
				return journal.ErrTransition
			}
			return nil
		}
		if observed.Schema != "lycheedev.report-observation.v1" ||
			record.Stage == "dispatch_requested" || record.Stage == "flush_requested" {
			return journal.ErrTransition
		}
		_, err := evidence.OpenArchive(store, metadata).ReadVerifiedReport(ctx, observed.BodyID, observed.ReceiptID, input.Code, input.Expected, id, record.Intent.Snapshot)
		return err
	}
	var retirement *queueRetirement
	advance := func(stage, status string) error {
		// Preserve the entire original observation, including dispatch and capture
		// evidence. Both report and load schemas must survive interrupted cleanup.
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(record.Observation, &fields); err != nil {
			return err
		}
		if retirement != nil {
			raw, err := json.Marshal(retirement)
			if err != nil {
				return err
			}
			fields["queueRetirement"] = raw
		}
		raw, err := json.Marshal(fields)
		if err != nil {
			return err
		}
		if err := book.AdvanceStage(ctx, journal.StageChange{OperationID: id, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: raw}); err != nil {
			return err
		}
		record, err = book.InspectWork(ctx, id)
		return err
	}
	if record.Stage == "abandoned" && record.Status == "abandoned" {
		if err := verify(); err != nil {
			return record, err
		}
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
	if err := verify(); err != nil {
		return record, err
	}
	if record.Stage != "abandoning" {
		if err := advance("abandoning", "running"); err != nil {
			return record, err
		}
	}
	if _, err := run.Check(ctx); err != nil {
		return record, err
	}
	revision, err := delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(input.Load.Installation), definition, true)
	if err != nil {
		return record, err
	}
	retirement = &queueRetirement{Revision: revision}
	if err := advance("abandoned", "abandoned"); err != nil {
		return record, err
	}
	if err := run.Close(); err != nil {
		return record, err
	}
	return record, book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, id)
}
