package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
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

// Abandon is a user decision to stop recovery, never evidence that input ran or
// that the runtime is clean. In particular, partial and unsent input must not
// trap a disk owner forever. Prepared work still uses Cancel.
func abandonableProbeStage(stage string) bool {
	switch stage {
	case "load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted", "verified", "ack_requested":
		return true
	}
	return false
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
	if record.Intent.Kind != "probe" && record.Intent.Kind != "faults" {
		return record, journal.ErrTransition
	}
	// Explicit abandonment retires host ownership without inferring execution.
	if !abandonableProbeStage(record.Stage) && record.Stage != "abandoning" && record.Stage != "abandoned" {
		return record, journal.ErrTransition
	}
	var input ReportIntent
	var definition bridge.ProbeDefinition
	if record.Intent.Kind == "faults" {
		input, _, err = faultInput(record)
	} else {
		input, definition, err = probeDefinition(record)
	}
	if err != nil {
		return record, err
	}
	parent := filepath.Join(input.Load.Installation, "Interface", "AddOns")
	verify := func() error {
		var observed reportObservation
		if err := json.Unmarshal(record.Observation, &observed); err != nil {
			return err
		}
		if observed.AcknowledgementID != "" {
			return journal.ErrTransition
		}
		if observed.Schema == "lycheedev.probe-load.v1" || record.Intent.Kind == "faults" && observed.Schema == "lycheedev.fault-observation.v1" {
			if record.Stage == "verified" || record.Stage == "ack_requested" || observed.BodyID != "" || observed.ReceiptID != "" {
				return journal.ErrTransition
			}
			return nil
		}
		if observed.Schema != "lycheedev.report-observation.v1" {
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
	var revision delivery.QueueRevision
	if record.Intent.Kind == "probe" {
		revision, err = delivery.ChangeProbeQueue(ctx, delivery.AddonDirectory(input.Load.Installation), definition, true)
	}
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
